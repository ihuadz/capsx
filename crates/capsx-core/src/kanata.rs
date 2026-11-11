//! kanata 子进程启动、监督、退避重启与失败熔断。
//!
//! 监督线程是唯一持有/轮询 `Child` 的地方；UI 线程通过原子标志请求
//! 停止或手动重启，避免在 UI 线程等待子进程。

use std::collections::VecDeque;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::constants::{
    FAST_FAILURE_WINDOW_MS, MAX_FAST_FAILURES, PROCESS_STABLE_MS, RESTART_BACKOFF_MS,
    TCP_STARTUP_DEADLINE_MS,
};
use crate::error::{Error, Result};
use crate::platform;

const MONITOR_TICK_MS: u64 = 100;
const MANUAL_RESTART_DELAY_MS: u64 = 300;
const LOG_TAIL_LINES: usize = 12;

/// 一次 kanata 运行所需的参数。
#[derive(Debug, Clone)]
pub struct SupervisorConfig {
    /// `kanata` 可执行文件。
    pub kanata: PathBuf,
    /// 已校验的 `keymap.kbd`。
    pub config: PathBuf,
    /// 本地 TCP 端口。
    pub port: u16,
}

impl SupervisorConfig {
    /// `127.0.0.1:<port>`。
    pub fn address(&self) -> String {
        format!("127.0.0.1:{}", self.port)
    }
}

/// 监督线程发出的事件。
#[derive(Debug, Clone)]
pub enum SupervisorEvent {
    /// 子进程已启动。
    Started {
        /// 进程号。
        pid: u32,
    },
    /// 子进程退出。
    Exited {
        /// 退出码。
        code: Option<i32>,
        /// 是否是用户主动停止/重启。
        expected: bool,
    },
    /// 即将退避重启。
    Restarting {
        /// 第几次连续失败（普通重启为 0）。
        attempt: usize,
        /// 延迟毫秒。
        delay_ms: u64,
    },
    /// 连续失败达到熔断条件，不再重启。
    Failed {
        /// 可供 UI 展示的原因。
        reason: String,
    },
    /// 监督线程结束。
    Stopped,
}

/// 监督 kanata 子进程的后台线程句柄。
pub struct KanataSupervisor {
    stop: Arc<AtomicBool>,
    restart: Arc<AtomicBool>,
    child: Arc<Mutex<Option<Child>>>,
    worker: Option<JoinHandle<()>>,
}

impl KanataSupervisor {
    /// 启动监督线程。
    pub fn start(
        config: SupervisorConfig,
        tcp_connected: Arc<AtomicBool>,
        events: Sender<SupervisorEvent>,
    ) -> Result<Self> {
        if !config.kanata.is_file() {
            return Err(Error::MissingKanata(config.kanata.clone()));
        }
        if !config.config.is_file() {
            return Err(Error::ConfigInstall(format!(
                "找不到配置文件：{}",
                config.config.display()
            )));
        }

        let stop = Arc::new(AtomicBool::new(false));
        let restart = Arc::new(AtomicBool::new(false));
        let child = Arc::new(Mutex::new(None));

        let worker_stop = Arc::clone(&stop);
        let worker_restart = Arc::clone(&restart);
        let worker_child = Arc::clone(&child);
        let worker = thread::Builder::new()
            .name("capsx-kanata-supervisor".into())
            .spawn(move || {
                supervise(
                    config,
                    tcp_connected,
                    events,
                    worker_stop,
                    worker_restart,
                    worker_child,
                );
            })
            .map_err(|error| Error::Process(format!("启动监督线程失败：{error}")))?;

        Ok(Self {
            stop,
            restart,
            child,
            worker: Some(worker),
        })
    }

    /// 请求重启 kanata（用户主动操作，不计入失败次数）。
    pub fn restart(&self) {
        self.restart.store(true, Ordering::SeqCst);
        let mut guard = match self.child.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Some(child) = guard.as_mut() {
            if let Err(error) = child.kill() {
                log::warn!("主动结束 kanata 失败：{error}");
            }
        }
    }

    /// 停止监督并结束子进程。幂等。
    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.restart.store(false, Ordering::SeqCst);
        {
            let mut guard = match self.child.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            if let Some(child) = guard.as_mut() {
                if let Err(error) = child.kill() {
                    log::debug!("停止 kanata 时 kill 返回：{error}");
                }
            }
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for KanataSupervisor {
    fn drop(&mut self) {
        self.stop();
    }
}

/// 退避序列的第 `index` 项。
pub fn backoff_delay(index: usize) -> Duration {
    let millis = RESTART_BACKOFF_MS
        .get(index)
        .copied()
        .unwrap_or(*RESTART_BACKOFF_MS.last().unwrap_or(&30_000));
    Duration::from_millis(millis)
}

fn supervise(
    config: SupervisorConfig,
    tcp_connected: Arc<AtomicBool>,
    events: Sender<SupervisorEvent>,
    stop: Arc<AtomicBool>,
    restart: Arc<AtomicBool>,
    child_slot: Arc<Mutex<Option<Child>>>,
) {
    let mut failures: VecDeque<Instant> = VecDeque::new();
    let mut backoff_index = 0_usize;
    let log_tail: Arc<Mutex<VecDeque<String>>> = Arc::new(Mutex::new(VecDeque::new()));

    loop {
        if stop.load(Ordering::SeqCst) {
            break;
        }

        let started_at = Instant::now();
        let mut startup_tcp_failed = false;
        let mut manual_restart = false;

        let spawn_result = spawn_child(&config, &log_tail);
        match spawn_result {
            Ok(child) => {
                let pid = child.id();
                let cleanup = match platform::attach_child_cleanup(&child) {
                    Ok(cleanup) => Some(cleanup),
                    Err(error) => {
                        log::warn!("kanata 子进程缺少异常清理保护：{error}");
                        None
                    }
                };

                {
                    let mut guard = match child_slot.lock() {
                        Ok(guard) => guard,
                        Err(poisoned) => poisoned.into_inner(),
                    };
                    *guard = Some(child);
                }
                let _ = events.send(SupervisorEvent::Started { pid });
                log::info!("kanata 已启动：pid={pid}，port={}", config.port);

                let mut connected_once = tcp_connected.load(Ordering::SeqCst);
                let mut exit_code = None;
                loop {
                    if stop.load(Ordering::SeqCst) {
                        break;
                    }
                    if restart.swap(false, Ordering::SeqCst) {
                        manual_restart = true;
                        kill_slot(&child_slot);
                    }

                    let status = {
                        let mut guard = match child_slot.lock() {
                            Ok(guard) => guard,
                            Err(poisoned) => poisoned.into_inner(),
                        };
                        match guard.as_mut() {
                            Some(child) => match child.try_wait() {
                                Ok(status) => status,
                                Err(error) => {
                                    log::warn!("try_wait 失败：{error}");
                                    None
                                }
                            },
                            None => None,
                        }
                    };

                    if let Some(status) = status {
                        exit_code = status.code();
                        break;
                    }

                    if tcp_connected.load(Ordering::SeqCst) {
                        connected_once = true;
                    }
                    let elapsed_ms = started_at.elapsed().as_millis() as u64;
                    if !connected_once && elapsed_ms >= TCP_STARTUP_DEADLINE_MS {
                        startup_tcp_failed = true;
                        log::error!(
                            "kanata 启动后 {TCP_STARTUP_DEADLINE_MS} ms 内未建立 TCP 连接，终止并重启"
                        );
                        kill_slot(&child_slot);
                    }

                    thread::sleep(Duration::from_millis(MONITOR_TICK_MS));
                }

                let exited = {
                    let mut guard = match child_slot.lock() {
                        Ok(guard) => guard,
                        Err(poisoned) => poisoned.into_inner(),
                    };
                    guard.take()
                };
                if let Some(mut child) = exited {
                    let _ = child.wait();
                }
                drop(cleanup);

                let _ = events.send(SupervisorEvent::Exited {
                    code: exit_code,
                    expected: stop.load(Ordering::SeqCst) || manual_restart,
                });

                if stop.load(Ordering::SeqCst) {
                    break;
                }

                if manual_restart {
                    failures.clear();
                    backoff_index = 0;
                    let _ = events.send(SupervisorEvent::Restarting {
                        attempt: 0,
                        delay_ms: MANUAL_RESTART_DELAY_MS,
                    });
                    let _ = wait_interruptible(
                        Duration::from_millis(MANUAL_RESTART_DELAY_MS),
                        &stop,
                        &restart,
                    );
                    continue;
                }
            }
            Err(error) => {
                log::error!("启动 kanata 失败：{error}");
                if stop.load(Ordering::SeqCst) {
                    break;
                }
            }
        }

        let elapsed = started_at.elapsed();
        let early_failure = !manual_restart
            && (startup_tcp_failed || elapsed < Duration::from_millis(PROCESS_STABLE_MS));

        if early_failure {
            failures.push_back(Instant::now());
            while let Some(front) = failures.front().copied() {
                if front.elapsed() > Duration::from_millis(FAST_FAILURE_WINDOW_MS) {
                    failures.pop_front();
                } else {
                    break;
                }
            }

            if failures.len() >= MAX_FAST_FAILURES {
                let tail = tail_text(&log_tail);
                let mut reason = format!(
                    "kanata 在 {} 秒内连续失败 {} 次，已停止自动重启。",
                    FAST_FAILURE_WINDOW_MS / 1_000,
                    failures.len()
                );
                if startup_tcp_failed {
                    reason.push_str(" 最后一次是 10 秒内未建立 TCP 连接。");
                }
                if !tail.is_empty() {
                    reason.push_str("\n\nkanata 输出：\n");
                    reason.push_str(&tail);
                }
                let _ = events.send(SupervisorEvent::Failed { reason });
                log::error!("kanata 连续失败，监督线程停止");
                break;
            }
        } else {
            failures.clear();
            backoff_index = 0;
        }

        let delay = if manual_restart {
            Duration::from_millis(MANUAL_RESTART_DELAY_MS)
        } else if early_failure {
            backoff_delay(backoff_index)
        } else {
            backoff_delay(0)
        };
        if early_failure {
            backoff_index = (backoff_index + 1).min(RESTART_BACKOFF_MS.len() - 1);
        }

        let _ = events.send(SupervisorEvent::Restarting {
            attempt: failures.len(),
            delay_ms: delay.as_millis() as u64,
        });
        log::warn!(
            "kanata 将在 {} ms 后重启（近期失败 {} 次）",
            delay.as_millis(),
            failures.len()
        );

        if wait_interruptible(delay, &stop, &restart) == WaitKind::Stop {
            break;
        }
    }

    kill_slot(&child_slot);
    let _ = events.send(SupervisorEvent::Stopped);
    log::info!("kanata 监督线程结束");
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WaitKind {
    Elapsed,
    ManualRestart,
    Stop,
}

fn wait_interruptible(duration: Duration, stop: &AtomicBool, restart: &AtomicBool) -> WaitKind {
    let deadline = Instant::now() + duration;
    loop {
        if stop.load(Ordering::SeqCst) {
            return WaitKind::Stop;
        }
        if restart.swap(false, Ordering::SeqCst) {
            return WaitKind::ManualRestart;
        }
        let now = Instant::now();
        if now >= deadline {
            return WaitKind::Elapsed;
        }
        thread::sleep((deadline - now).min(Duration::from_millis(100)));
    }
}

fn spawn_child(
    config: &SupervisorConfig,
    log_tail: &Arc<Mutex<VecDeque<String>>>,
) -> Result<Child> {
    let mut command = Command::new(&config.kanata);
    command
        .arg("-c")
        .arg(&config.config)
        .arg("-p")
        .arg(config.address())
        .arg("-q")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    platform::hide_console(&mut command);

    let mut child = command.spawn().map_err(|error| {
        Error::Process(format!("无法启动 {}：{error}", config.kanata.display()))
    })?;

    if let Some(stdout) = child.stdout.take() {
        spawn_log_reader(stdout, false, Arc::clone(log_tail));
    }
    if let Some(stderr) = child.stderr.take() {
        spawn_log_reader(stderr, true, Arc::clone(log_tail));
    }

    Ok(child)
}

fn spawn_log_reader(
    stream: impl std::io::Read + Send + 'static,
    is_error: bool,
    tail: Arc<Mutex<VecDeque<String>>>,
) {
    let _ = thread::Builder::new()
        .name(if is_error {
            "capsx-kanata-stderr".into()
        } else {
            "capsx-kanata-stdout".into()
        })
        .spawn(move || {
            for line in BufReader::new(stream)
                .lines()
                .map_while(std::result::Result::ok)
            {
                if is_error {
                    log::warn!(target: "kanata", "{line}");
                } else {
                    log::info!(target: "kanata", "{line}");
                }
                if let Ok(mut guard) = tail.lock() {
                    guard.push_back(line);
                    while guard.len() > LOG_TAIL_LINES {
                        guard.pop_front();
                    }
                }
            }
        });
}

fn tail_text(tail: &Arc<Mutex<VecDeque<String>>>) -> String {
    let guard = match tail.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    guard.iter().cloned().collect::<Vec<_>>().join("\n")
}

fn kill_slot(slot: &Arc<Mutex<Option<Child>>>) {
    let mut guard = match slot.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    if let Some(child) = guard.as_mut() {
        if let Err(error) = child.kill() {
            log::debug!("结束 kanata 子进程失败：{error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_sequence_is_capped() {
        assert_eq!(backoff_delay(0), Duration::from_secs(1));
        assert_eq!(backoff_delay(1), Duration::from_secs(2));
        assert_eq!(backoff_delay(2), Duration::from_secs(4));
        assert_eq!(backoff_delay(5), Duration::from_secs(30));
        assert_eq!(backoff_delay(99), Duration::from_secs(30));
    }
}
