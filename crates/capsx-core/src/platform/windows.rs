//! Windows 平台实现。

use std::ffi::c_void;
use std::io;
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::ptr;
use std::thread;
use std::time::Duration;

use crate::constants::{ACTIVATE_EVENT, QUIT_EVENT};
use crate::error::{Error, Result};
use crate::platform::{Autostart, ChildCleanup, LocalNow};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_VALUE: &str = "CapsX";

const EVENT_MODIFY_STATE: u32 = 0x0002;
const WAIT_OBJECT_0: u32 = 0;
const WAIT_TIMEOUT: u32 = 0x0000_0102;

const JOB_OBJECT_EXTENDED_LIMIT_INFORMATION_CLASS: u32 = 9;
const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: u32 = 0x0000_2000;

type Handle = *mut c_void;

#[link(name = "kernel32")]
extern "system" {
    fn CreateEventW(
        event_attributes: *mut c_void,
        manual_reset: i32,
        initial_state: i32,
        name: *const u16,
    ) -> Handle;
    fn OpenEventW(desired_access: u32, inherit_handle: i32, name: *const u16) -> Handle;
    fn SetEvent(event: Handle) -> i32;
    fn WaitForSingleObject(handle: Handle, milliseconds: u32) -> u32;
    fn CloseHandle(handle: Handle) -> i32;

    fn CreateJobObjectW(attributes: *mut c_void, name: *const u16) -> Handle;
    fn SetInformationJobObject(
        job: Handle,
        info_class: u32,
        info: *mut c_void,
        info_length: u32,
    ) -> i32;
    fn AssignProcessToJobObject(job: Handle, process: Handle) -> i32;

    fn GetLocalTime(system_time: *mut SystemTime);
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct SystemTime {
    year: u16,
    month: u16,
    day_of_week: u16,
    day: u16,
    hour: u16,
    minute: u16,
    second: u16,
    millis: u16,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct JobBasicLimitInformation {
    per_process_user_time_limit: i64,
    per_job_user_time_limit: i64,
    limit_flags: u32,
    minimum_working_set_size: usize,
    maximum_working_set_size: usize,
    active_process_limit: u32,
    affinity: usize,
    priority_class: u32,
    scheduling_class: u32,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct IoCounters {
    read_operation_count: u64,
    write_operation_count: u64,
    other_operation_count: u64,
    read_transfer_count: u64,
    write_transfer_count: u64,
    other_transfer_count: u64,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct JobExtendedLimitInformation {
    basic_limit_information: JobBasicLimitInformation,
    io_info: IoCounters,
    process_memory_limit: usize,
    job_memory_limit: usize,
    peak_process_memory_used: usize,
    peak_job_memory_used: usize,
}

fn wide(name: &str) -> Vec<u16> {
    name.encode_utf16().chain(std::iter::once(0)).collect()
}

struct EventHandle(Handle);

// HANDLE 只在内核对象上使用；跨线程传递由 Windows 的事件对象语义保证。
unsafe impl Send for EventHandle {}
unsafe impl Sync for EventHandle {}

impl EventHandle {
    fn create(name: &str) -> io::Result<Self> {
        let name = wide(name);
        // 手动复位 = 0，自动复位事件；连续两次唤出会合并成一次。
        let handle = unsafe { CreateEventW(ptr::null_mut(), 0, 0, name.as_ptr()) };
        if handle.is_null() {
            Err(io::Error::last_os_error())
        } else {
            Ok(Self(handle))
        }
    }

    fn open(name: &str) -> io::Result<Self> {
        let name = wide(name);
        let handle = unsafe { OpenEventW(EVENT_MODIFY_STATE, 0, name.as_ptr()) };
        if handle.is_null() {
            Err(io::Error::last_os_error())
        } else {
            Ok(Self(handle))
        }
    }

    fn signal(&self) -> io::Result<()> {
        if unsafe { SetEvent(self.0) } == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    /// 阻塞等待事件；超时返回 `false`。
    fn wait(&self, timeout: Duration) -> io::Result<bool> {
        let millis = timeout.as_millis().min(u32::MAX as u128) as u32;
        match unsafe { WaitForSingleObject(self.0, millis) } {
            WAIT_OBJECT_0 => Ok(true),
            WAIT_TIMEOUT => Ok(false),
            _ => Err(io::Error::last_os_error()),
        }
    }
}

impl Drop for EventHandle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

struct WindowsAutostart;

impl Autostart for WindowsAutostart {
    fn is_enabled(&self) -> Result<bool> {
        use winreg::enums::HKEY_CURRENT_USER;
        use winreg::RegKey;

        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let key = match hkcu.open_subkey(RUN_KEY) {
            Ok(key) => key,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(error) => {
                return Err(Error::Platform(format!("读取自启注册表失败：{error}")));
            }
        };
        match key.get_value::<String, _>(RUN_VALUE) {
            Ok(value) => Ok(!value.trim().is_empty()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(Error::Platform(format!("读取自启项失败：{error}"))),
        }
    }

    fn set_enabled(&self, enabled: bool) -> Result<()> {
        use winreg::enums::HKEY_CURRENT_USER;
        use winreg::RegKey;

        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let (key, _) = hkcu
            .create_subkey(RUN_KEY)
            .map_err(|error| Error::Platform(format!("打开自启注册表失败：{error}")))?;

        if enabled {
            let exe = std::env::current_exe()?;
            let command = format!("\"{}\" --tray", exe.display());
            key.set_value(RUN_VALUE, &command)
                .map_err(|error| Error::Platform(format!("写入自启项失败：{error}")))?;
        } else {
            match key.delete_value(RUN_VALUE) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(Error::Platform(format!("删除自启项失败：{error}")));
                }
            }
        }
        Ok(())
    }
}

pub(super) fn autostart() -> Box<dyn Autostart> {
    Box::new(WindowsAutostart)
}

pub(super) fn install_instance_listeners(
    on_activate: Box<dyn Fn() + Send>,
    on_quit: Box<dyn Fn() + Send>,
) -> Result<()> {
    let activate = EventHandle::create(ACTIVATE_EVENT)
        .map_err(|error| Error::Platform(format!("创建激活事件失败：{error}")))?;
    let quit = EventHandle::create(QUIT_EVENT)
        .map_err(|error| Error::Platform(format!("创建退出事件失败：{error}")))?;

    thread::Builder::new()
        .name("capsx-activate".into())
        .spawn(move || loop {
            match activate.wait(Duration::from_millis(500)) {
                Ok(true) => on_activate(),
                Ok(false) => {}
                Err(error) => {
                    log::warn!("等待激活事件失败：{error}");
                    thread::sleep(Duration::from_secs(1));
                }
            }
        })
        .map_err(|error| Error::Platform(format!("启动激活监听线程失败：{error}")))?;

    thread::Builder::new()
        .name("capsx-quit".into())
        .spawn(move || loop {
            match quit.wait(Duration::from_millis(500)) {
                Ok(true) => on_quit(),
                Ok(false) => {}
                Err(error) => {
                    log::warn!("等待退出事件失败：{error}");
                    thread::sleep(Duration::from_secs(1));
                }
            }
        })
        .map_err(|error| Error::Platform(format!("启动退出监听线程失败：{error}")))?;

    Ok(())
}

pub(super) fn signal_event(name: &str) -> Result<()> {
    // 第二实例启动时第一实例可能还没建好事件，最多等 2 秒。
    let mut last_error = None;
    for _ in 0..20 {
        match EventHandle::open(name) {
            Ok(event) => {
                return event
                    .signal()
                    .map_err(|error| Error::Platform(format!("发送实例事件失败：{error}")));
            }
            Err(error) => {
                last_error = Some(error);
                thread::sleep(Duration::from_millis(100));
            }
        }
    }
    Err(Error::Platform(format!(
        "找不到已运行实例的事件对象：{}",
        last_error
            .map(|error| error.to_string())
            .unwrap_or_else(|| "未知错误".into())
    )))
}

pub(super) struct ProcessJob {
    handle: Handle,
}

impl ProcessJob {
    fn new_kill_on_close() -> io::Result<Self> {
        let handle = unsafe { CreateJobObjectW(ptr::null_mut(), ptr::null()) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }

        let mut info = JobExtendedLimitInformation::default();
        info.basic_limit_information.limit_flags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let ok = unsafe {
            SetInformationJobObject(
                handle,
                JOB_OBJECT_EXTENDED_LIMIT_INFORMATION_CLASS,
                (&mut info as *mut JobExtendedLimitInformation).cast(),
                std::mem::size_of::<JobExtendedLimitInformation>() as u32,
            )
        };
        if ok == 0 {
            let error = io::Error::last_os_error();
            unsafe {
                CloseHandle(handle);
            }
            return Err(error);
        }

        Ok(Self { handle })
    }

    fn assign(&self, child: &std::process::Child) -> io::Result<()> {
        let process = child.as_raw_handle() as Handle;
        let ok = unsafe { AssignProcessToJobObject(self.handle, process) };
        if ok == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}

impl Drop for ProcessJob {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.handle);
        }
    }
}

pub(super) fn attach_child_cleanup(child: &std::process::Child) -> Result<ChildCleanup> {
    let job = ProcessJob::new_kill_on_close()
        .map_err(|error| Error::Process(format!("创建 Job Object 失败：{error}")))?;
    if let Err(error) = job.assign(child) {
        // 拿不到 Job 不应该阻断主功能；父进程正常退出时仍会显式 kill。
        log::warn!("kanata 加入 Job Object 失败（异常退出时可能留下子进程）：{error}");
    }
    Ok(ChildCleanup { _job: job })
}

pub(super) fn hide_console(command: &mut std::process::Command) {
    command.creation_flags(CREATE_NO_WINDOW);
}

pub(super) fn local_now() -> LocalNow {
    let mut time = SystemTime::default();
    unsafe {
        GetLocalTime(&mut time);
    }
    LocalNow {
        year: time.year as i32,
        month: time.month as u32,
        day: time.day as u32,
        hour: time.hour as u32,
        minute: time.minute as u32,
        second: time.second as u32,
        millis: time.millis as u32,
    }
}
