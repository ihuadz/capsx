//! kanata TCP JSON 行协议客户端。
//!
//! 设计要点：
//! * 单后台线程同时负责“发命令”和“读事件”，UI 线程永不阻塞；
//! * 断线后 3 秒重连，最多 5 次；可通过 [`KanataClient::reconnect`] 手动重置；
//! * 使用增量字节缓冲解析换行分隔 JSON，避免 `read_line` 在超时后丢半个包。

use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::Value;

use crate::constants::{
    TCP_CONNECT_TIMEOUT_MS, TCP_HELLO_TIMEOUT_MS, TCP_INITIAL_MAX_ATTEMPTS,
    TCP_INITIAL_RETRY_DELAY_MS, TCP_MAX_RECONNECTS, TCP_POLL_INTERVAL_MS, TCP_RECONNECT_DELAY_MS,
};
use crate::error::{Error, Result};

const MAX_LINE_BYTES: usize = 1024 * 1024;

/// 客户端时间参数（测试可缩短）。
#[derive(Debug, Clone)]
pub struct ClientOptions {
    /// TCP 连接超时。
    pub connect_timeout: Duration,
    /// 等待 `HelloOk` 的超时。
    pub hello_timeout: Duration,
    /// 重连间隔。
    pub reconnect_delay: Duration,
    /// 首次连接（尚未成功过）的重试间隔。
    pub initial_retry_delay: Duration,
    /// 最大重连次数（成功连接过一次之后）。
    pub max_reconnects: u32,
    /// 首次连接的最大尝试次数（成功连接之前）。
    pub initial_max_attempts: u32,
    /// 读轮询间隔。
    pub poll_interval: Duration,
}

impl Default for ClientOptions {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_millis(TCP_CONNECT_TIMEOUT_MS),
            hello_timeout: Duration::from_millis(TCP_HELLO_TIMEOUT_MS),
            reconnect_delay: Duration::from_millis(TCP_RECONNECT_DELAY_MS),
            initial_retry_delay: Duration::from_millis(TCP_INITIAL_RETRY_DELAY_MS),
            max_reconnects: TCP_MAX_RECONNECTS,
            initial_max_attempts: TCP_INITIAL_MAX_ATTEMPTS,
            poll_interval: Duration::from_millis(TCP_POLL_INTERVAL_MS),
        }
    }
}

/// 从 kanata 收到的一条事件。
#[derive(Debug, Clone)]
pub enum ClientEvent {
    /// TCP 连接和 `Hello` 握手均完成。
    Connected {
        /// kanata 版本。
        version: String,
    },
    /// 连接断开。
    Disconnected {
        /// 原因。
        reason: String,
    },
    /// 正在退避重连。
    Reconnecting {
        /// 第几次重连。
        attempt: u32,
        /// 最大次数。
        max: u32,
        /// 等待毫秒数。
        delay_ms: u64,
    },
    /// 重连次数耗尽，等待手动重试。
    ReconnectExhausted {
        /// 最后一次错误。
        reason: String,
    },
    /// `HelloOk`。
    HelloOk {
        /// kanata 版本。
        version: String,
        /// 协议版本。
        protocol: Option<u64>,
    },
    /// `MessagePush`，用于 `CapsLock + ,`。
    MessagePush {
        /// 消息内容。
        message: String,
        /// 原始 JSON。
        raw: String,
    },
    /// 当前层改变。
    LayerChange {
        /// 新层名。
        new: String,
        /// 原始 JSON。
        raw: String,
    },
    /// 配置文件重载事件。
    ConfigFileReload {
        /// 新配置文件路径。
        new: String,
        /// 原始 JSON。
        raw: String,
    },
    /// kanata 报错。
    KanataError {
        /// 错误文本。
        message: String,
        /// 原始 JSON。
        raw: String,
    },
    /// Reload 结果（关键路径）。
    ReloadResult {
        /// 是否成功。
        ok: bool,
        /// 原始 JSON。
        raw: String,
    },
    /// 当前层名。
    CurrentLayerName {
        /// 层名。
        name: String,
        /// 原始 JSON。
        raw: String,
    },
    /// 协议状态（例如 `{"status":"Ok"}`）。
    Status {
        /// 状态文本。
        status: String,
        /// 原始 JSON。
        raw: String,
    },
    /// 调试事件。
    HoldActivated {
        /// 原始 JSON。
        raw: String,
    },
    /// 调试事件。
    TapActivated {
        /// 原始 JSON。
        raw: String,
    },
    /// 未识别但合法的 JSON。
    Unknown {
        /// 原始 JSON。
        raw: String,
    },
    /// 无法解析的行。
    ParseError {
        /// 原始行。
        line: String,
        /// 错误。
        error: String,
    },
    /// 连接未建立时命令被丢弃。
    CommandDropped {
        /// 原因。
        reason: String,
    },
}

/// 与运行中的 kanata 通信的客户端。
pub struct KanataClient {
    command_tx: Sender<WorkerCommand>,
    event_rx: Receiver<ClientEvent>,
    connected: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl KanataClient {
    /// 启动后台连接线程（不会阻塞）。
    pub fn start(address: SocketAddr) -> Result<Self> {
        Self::start_with_options(address, ClientOptions::default())
    }

    /// 带自定义时间参数启动。
    pub fn start_with_options(address: SocketAddr, options: ClientOptions) -> Result<Self> {
        let (command_tx, command_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();
        let connected = Arc::new(AtomicBool::new(false));
        let shutdown = Arc::new(AtomicBool::new(false));

        let worker_connected = Arc::clone(&connected);
        let worker_shutdown = Arc::clone(&shutdown);
        let worker = thread::Builder::new()
            .name("capsx-kanata-tcp".into())
            .spawn(move || {
                worker_loop(
                    address,
                    options,
                    command_rx,
                    event_tx,
                    worker_connected,
                    worker_shutdown,
                );
            })
            .map_err(|error| Error::Process(format!("启动 TCP 线程失败：{error}")))?;

        Ok(Self {
            command_tx,
            event_rx,
            connected,
            shutdown,
            worker: Some(worker),
        })
    }

    /// 连接状态标志，可交给 supervisor 做 10 秒启动判定。
    pub fn connected_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.connected)
    }

    /// 当前是否已连接。
    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }

    /// 非阻塞取一条事件。
    pub fn poll_event(&self) -> Option<ClientEvent> {
        self.event_rx.try_recv().ok()
    }

    /// 事件接收器（供需要 select 的调用方使用）。
    pub fn event_receiver(&self) -> &Receiver<ClientEvent> {
        &self.event_rx
    }

    /// 发送 `Hello`（正常由后台线程握手时自动发送）。
    pub fn send_hello(&self) -> Result<()> {
        self.send_json(&Outgoing::Hello(Empty {}), false)
    }

    /// 请求重载配置。
    pub fn reload(&self, wait: bool, timeout_ms: u64) -> Result<()> {
        self.send_json(&Outgoing::Reload(ReloadRequest { wait, timeout_ms }), wait)
    }

    /// 请求当前层名。
    pub fn request_current_layer_name(&self) -> Result<()> {
        self.send_json(&Outgoing::RequestCurrentLayerName(Empty {}), false)
    }

    /// 切换层（M3 按应用切层预留）。
    pub fn change_layer(&self, new: &str) -> Result<()> {
        self.send_json(&Outgoing::ChangeLayer(ChangeLayerRequest { new }), false)
    }

    /// 触发虚键（可选）。
    pub fn act_on_fake_key(&self, name: &str, action: &str) -> Result<()> {
        self.send_json(
            &Outgoing::ActOnFakeKey(FakeKeyRequest { name, action }),
            false,
        )
    }

    /// 手动立即重连并清零失败计数。
    pub fn reconnect(&self) -> Result<()> {
        self.command_tx
            .send(WorkerCommand::Reconnect)
            .map_err(|_| Error::Tcp("TCP 工作线程已退出".into()))
    }

    /// 主动停止后台线程。
    pub fn shutdown(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        let _ = self.command_tx.send(WorkerCommand::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        self.connected.store(false, Ordering::SeqCst);
    }

    fn send_json(&self, message: &Outgoing<'_>, wake: bool) -> Result<()> {
        let line = serde_json::to_string(message)?;
        self.command_tx
            .send(WorkerCommand::Send { line, wake })
            .map_err(|_| Error::Tcp("TCP 工作线程已退出".into()))
    }
}

impl Drop for KanataClient {
    fn drop(&mut self) {
        self.shutdown();
    }
}

enum WorkerCommand {
    Send { line: String, wake: bool },
    Reconnect,
    Shutdown,
}

#[derive(Serialize)]
struct Empty {}

#[derive(Serialize)]
struct ReloadRequest {
    wait: bool,
    timeout_ms: u64,
}

#[derive(Serialize)]
struct ChangeLayerRequest<'a> {
    new: &'a str,
}

#[derive(Serialize)]
struct FakeKeyRequest<'a> {
    name: &'a str,
    action: &'a str,
}

#[derive(Serialize)]
enum Outgoing<'a> {
    Hello(Empty),
    Reload(ReloadRequest),
    RequestCurrentLayerName(Empty),
    ChangeLayer(ChangeLayerRequest<'a>),
    ActOnFakeKey(FakeKeyRequest<'a>),
}

fn worker_loop(
    address: SocketAddr,
    options: ClientOptions,
    command_rx: Receiver<WorkerCommand>,
    event_tx: Sender<ClientEvent>,
    connected: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
) {
    let mut retries: u32 = 0;
    let mut ever_connected = false;
    let mut last_error = String::new();

    'outer: loop {
        if shutdown.load(Ordering::SeqCst) {
            break;
        }

        if retries > 0 {
            let max_attempts = if ever_connected {
                options.max_reconnects.max(1)
            } else {
                options.initial_max_attempts.max(1)
            };
            if retries > max_attempts {
                emit(
                    &event_tx,
                    ClientEvent::ReconnectExhausted {
                        reason: last_error.clone(),
                    },
                );
                match wait_dormant(&command_rx, &event_tx, &shutdown) {
                    WaitOutcome::Stop => break,
                    WaitOutcome::Reset => {
                        retries = 0;
                        continue;
                    }
                    WaitOutcome::Continue => {
                        retries = 0;
                        continue;
                    }
                }
            }

            let delay = if ever_connected {
                options.reconnect_delay
            } else {
                options.initial_retry_delay
            };
            emit(
                &event_tx,
                ClientEvent::Reconnecting {
                    attempt: retries,
                    max: max_attempts,
                    delay_ms: delay.as_millis() as u64,
                },
            );
            match wait_for_retry(delay, &command_rx, &event_tx, &shutdown) {
                WaitOutcome::Stop => break,
                WaitOutcome::Reset => {
                    retries = 0;
                    continue;
                }
                WaitOutcome::Continue => {}
            }
        }

        if shutdown.load(Ordering::SeqCst) {
            break;
        }

        match connect_and_hello(address, &options, &event_tx) {
            Ok((session, version)) => {
                ever_connected = true;
                connected.store(true, Ordering::SeqCst);
                emit(&event_tx, ClientEvent::Connected { version });

                match serve(session, address, &command_rx, &event_tx, &shutdown) {
                    ServeOutcome::Shutdown => break,
                    ServeOutcome::Reconnect => {
                        connected.store(false, Ordering::SeqCst);
                        emit(
                            &event_tx,
                            ClientEvent::Disconnected {
                                reason: "手动重连".into(),
                            },
                        );
                        retries = 0;
                        continue;
                    }
                    ServeOutcome::Disconnected(reason) => {
                        connected.store(false, Ordering::SeqCst);
                        last_error = reason.clone();
                        emit(
                            &event_tx,
                            ClientEvent::Disconnected {
                                reason: reason.clone(),
                            },
                        );
                        retries = 1;
                        continue 'outer;
                    }
                }
            }
            Err(error) => {
                connected.store(false, Ordering::SeqCst);
                last_error = error.to_string();
                log::warn!("连接 kanata TCP 失败：{error}");
                retries = retries.saturating_add(1);
                let max_attempts = if ever_connected {
                    options.max_reconnects.max(1)
                } else {
                    options.initial_max_attempts.max(1)
                };
                if retries > max_attempts {
                    emit(
                        &event_tx,
                        ClientEvent::ReconnectExhausted {
                            reason: last_error.clone(),
                        },
                    );
                    match wait_dormant(&command_rx, &event_tx, &shutdown) {
                        WaitOutcome::Stop => break,
                        _ => {
                            retries = 0;
                            continue;
                        }
                    }
                }
            }
        }
    }

    connected.store(false, Ordering::SeqCst);
    log::debug!("kanata TCP 线程结束");
}

enum WaitOutcome {
    Continue,
    Reset,
    Stop,
}

fn wait_for_retry(
    delay: Duration,
    command_rx: &Receiver<WorkerCommand>,
    event_tx: &Sender<ClientEvent>,
    shutdown: &AtomicBool,
) -> WaitOutcome {
    let deadline = Instant::now() + delay;
    loop {
        if shutdown.load(Ordering::SeqCst) {
            return WaitOutcome::Stop;
        }
        let now = Instant::now();
        if now >= deadline {
            return WaitOutcome::Continue;
        }
        match command_rx.recv_timeout((deadline - now).min(Duration::from_millis(100))) {
            Ok(WorkerCommand::Shutdown) => return WaitOutcome::Stop,
            Ok(WorkerCommand::Reconnect) => return WaitOutcome::Reset,
            Ok(WorkerCommand::Send { .. }) => {
                emit(
                    event_tx,
                    ClientEvent::CommandDropped {
                        reason: "TCP 正在重连".into(),
                    },
                );
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return WaitOutcome::Stop,
        }
    }
}

fn wait_dormant(
    command_rx: &Receiver<WorkerCommand>,
    event_tx: &Sender<ClientEvent>,
    shutdown: &AtomicBool,
) -> WaitOutcome {
    loop {
        if shutdown.load(Ordering::SeqCst) {
            return WaitOutcome::Stop;
        }
        match command_rx.recv_timeout(Duration::from_millis(250)) {
            Ok(WorkerCommand::Shutdown) => return WaitOutcome::Stop,
            Ok(WorkerCommand::Reconnect) => return WaitOutcome::Reset,
            Ok(WorkerCommand::Send { .. }) => {
                emit(
                    event_tx,
                    ClientEvent::CommandDropped {
                        reason: "TCP 未连接".into(),
                    },
                );
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return WaitOutcome::Stop,
        }
    }
}

fn wake_kanata(address: SocketAddr) {
    let _ = thread::Builder::new()
        .name("capsx-kanata-wake".into())
        .spawn(move || {
            thread::sleep(Duration::from_millis(100));
            let mut stream = match TcpStream::connect_timeout(&address, Duration::from_millis(800))
            {
                Ok(stream) => stream,
                Err(error) => {
                    log::warn!("发送 Reload 唤醒连接失败：{error}");
                    return;
                }
            };
            let _ = stream.set_write_timeout(Some(Duration::from_millis(800)));
            let command = b"{\"RequestCurrentLayerName\":{}}\n";
            if let Err(error) = stream.write_all(command) {
                log::warn!("发送 Reload 唤醒命令失败：{error}");
                return;
            }
            let _ = stream.flush();
            // 给服务端线程一点时间读取并发送 WakeUp，然后再关闭。
            thread::sleep(Duration::from_millis(30));
        });
}

fn connect_and_hello(
    address: SocketAddr,
    options: &ClientOptions,
    event_tx: &Sender<ClientEvent>,
) -> Result<(Session, String)> {
    let stream = TcpStream::connect_timeout(&address, options.connect_timeout)
        .map_err(|error| Error::Tcp(format!("连接 {address} 失败：{error}")))?;
    stream.set_nodelay(true).ok();
    stream
        .set_read_timeout(Some(options.poll_interval))
        .map_err(|error| Error::Tcp(format!("设置读超时失败：{error}")))?;
    stream
        .set_write_timeout(Some(options.connect_timeout))
        .map_err(|error| Error::Tcp(format!("设置写超时失败：{error}")))?;

    let mut session = Session::new(stream);
    let hello = serde_json::to_string(&Outgoing::Hello(Empty {}))?;
    session.write_line(&hello)?;
    log::debug!("kanata TCP 已连接，已发送 Hello");

    let deadline = Instant::now() + options.hello_timeout;
    loop {
        match session.read_lines() {
            Ok(lines) => {
                for line in lines {
                    if let Some((version, protocol)) = dispatch_line(&line, event_tx) {
                        log::info!("kanata HelloOk：version={version} protocol={protocol:?}");
                        return Ok((session, version));
                    }
                }
            }
            Err(error) if is_timeout(&error) => {}
            Err(error) => {
                return Err(Error::Tcp(format!("等待 HelloOk 时读取失败：{error}")));
            }
        }
        if Instant::now() >= deadline {
            return Err(Error::Timeout(format!(
                "{} ms 内未收到 HelloOk",
                options.hello_timeout.as_millis()
            )));
        }
    }
}

fn serve(
    mut session: Session,
    address: SocketAddr,
    command_rx: &Receiver<WorkerCommand>,
    event_tx: &Sender<ClientEvent>,
    shutdown: &AtomicBool,
) -> ServeOutcome {
    loop {
        if shutdown.load(Ordering::SeqCst) {
            return ServeOutcome::Shutdown;
        }

        loop {
            match command_rx.try_recv() {
                Ok(WorkerCommand::Send { line, wake }) => {
                    if let Err(error) = session.write_line(&line) {
                        return ServeOutcome::Disconnected(format!("发送命令失败：{error}"));
                    }
                    if wake {
                        // kanata v1.12 的 TCP 线程在 wait:true 时会阻塞轮询
                        // is_reload_complete，而只有主循环的 WakeUp 才能推进。
                        // 用第二个短连接触发一次 WakeUp，避免白等 5 秒超时。
                        wake_kanata(address);
                    }
                }
                Ok(WorkerCommand::Reconnect) => return ServeOutcome::Reconnect,
                Ok(WorkerCommand::Shutdown) => return ServeOutcome::Shutdown,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return ServeOutcome::Shutdown,
            }
        }

        let lines = match session.read_lines() {
            Ok(lines) => lines,
            Err(error) if is_timeout(&error) => {
                // 读超时只用于轮询命令，不是断线。
                continue;
            }
            Err(error) => return ServeOutcome::Disconnected(format!("读取失败：{error}")),
        };
        for line in lines {
            let _ = dispatch_line(&line, event_tx);
        }
    }
}

enum ServeOutcome {
    Shutdown,
    Reconnect,
    Disconnected(String),
}

struct Session {
    stream: TcpStream,
    pending: Vec<u8>,
}

impl Session {
    fn new(stream: TcpStream) -> Self {
        Self {
            stream,
            pending: Vec::with_capacity(4096),
        }
    }

    fn write_line(&mut self, line: &str) -> Result<()> {
        log::debug!("TCP → {line}");
        self.stream
            .write_all(line.as_bytes())
            .and_then(|_| self.stream.write_all(b"\n"))
            .and_then(|_| self.stream.flush())
            .map_err(|error| Error::Tcp(format!("写入命令失败：{error}")))
    }

    fn read_lines(&mut self) -> io::Result<Vec<String>> {
        let mut temp = [0_u8; 8192];
        match self.stream.read(&mut temp) {
            Ok(0) => Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "kanata 关闭了 TCP 连接",
            )),
            Ok(count) => {
                self.pending.extend_from_slice(&temp[..count]);
                if self.pending.len() > MAX_LINE_BYTES {
                    self.pending.clear();
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "kanata 返回了超过 1 MB 的单行数据",
                    ));
                }
                Ok(self.take_lines())
            }
            Err(error) => Err(error),
        }
    }

    fn take_lines(&mut self) -> Vec<String> {
        let mut lines = Vec::new();
        while let Some(index) = self.pending.iter().position(|byte| *byte == b'\n') {
            let mut raw: Vec<u8> = self.pending.drain(..=index).collect();
            raw.pop();
            if raw.last() == Some(&b'\r') {
                raw.pop();
            }
            if raw.is_empty() {
                continue;
            }
            lines.push(String::from_utf8_lossy(&raw).into_owned());
        }
        lines
    }
}

fn is_timeout(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut | io::ErrorKind::Interrupted
    )
}

/// 解析一行并发出事件；如果是 `HelloOk`，返回版本信息。
fn dispatch_line(line: &str, event_tx: &Sender<ClientEvent>) -> Option<(String, Option<u64>)> {
    log::debug!("TCP ← {line}");
    let value: Value = match serde_json::from_str(line) {
        Ok(value) => value,
        Err(error) => {
            emit(
                event_tx,
                ClientEvent::ParseError {
                    line: line.to_string(),
                    error: error.to_string(),
                },
            );
            return None;
        }
    };

    let raw = line.to_string();
    if let Some(status) = value.get("status").and_then(Value::as_str) {
        emit(
            event_tx,
            ClientEvent::Status {
                status: status.to_string(),
                raw,
            },
        );
        return None;
    }

    if let Some(payload) = value.get("HelloOk") {
        let version = payload
            .get("version")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        let protocol = payload.get("protocol").and_then(Value::as_u64);
        emit(
            event_tx,
            ClientEvent::HelloOk {
                version: version.clone(),
                protocol,
            },
        );
        return Some((version, protocol));
    }

    if let Some(payload) = value.get("MessagePush") {
        let message = payload
            .get("message")
            .and_then(message_text)
            .unwrap_or_default();
        emit(
            event_tx,
            ClientEvent::MessagePush {
                message,
                raw: raw.clone(),
            },
        );
        return None;
    }

    if let Some(payload) = value.get("LayerChange") {
        let new = payload
            .get("new")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        emit(
            event_tx,
            ClientEvent::LayerChange {
                new,
                raw: raw.clone(),
            },
        );
        return None;
    }

    if let Some(payload) = value.get("ConfigFileReload") {
        let new = payload
            .get("new")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        emit(
            event_tx,
            ClientEvent::ConfigFileReload {
                new,
                raw: raw.clone(),
            },
        );
        return None;
    }

    if let Some(payload) = value.get("Error") {
        let message = payload
            .get("msg")
            .and_then(Value::as_str)
            .unwrap_or("kanata 未知错误")
            .to_string();
        emit(
            event_tx,
            ClientEvent::KanataError {
                message,
                raw: raw.clone(),
            },
        );
        return None;
    }

    if let Some(payload) = value.get("ReloadResult") {
        let ok = payload.get("ok").and_then(Value::as_bool).unwrap_or(false);
        emit(
            event_tx,
            ClientEvent::ReloadResult {
                ok,
                raw: raw.clone(),
            },
        );
        return None;
    }

    if let Some(payload) = value.get("CurrentLayerName") {
        let name = payload
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        emit(
            event_tx,
            ClientEvent::CurrentLayerName {
                name,
                raw: raw.clone(),
            },
        );
        return None;
    }

    if value.get("HoldActivated").is_some() {
        emit(event_tx, ClientEvent::HoldActivated { raw });
        return None;
    }
    if value.get("TapActivated").is_some() {
        emit(event_tx, ClientEvent::TapActivated { raw });
        return None;
    }

    emit(event_tx, ClientEvent::Unknown { raw });
    None
}

fn emit(event_tx: &Sender<ClientEvent>, event: ClientEvent) {
    let _ = event_tx.send(event);
}

/// kanata 文档写的是字符串，但 v1.12 实际会把 `push-msg` 的字符串序列化为
/// JSON 数组（`{"message":["capsx:settings"]}`）。两种形式都要兼容。
fn message_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Array(items) => Some(
            items
                .iter()
                .map(|item| match item {
                    Value::String(text) => text.clone(),
                    other => other.to_string(),
                })
                .collect::<Vec<_>>()
                .join(" "),
        ),
        Value::Null => None,
        other => Some(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    fn test_options() -> ClientOptions {
        ClientOptions {
            connect_timeout: Duration::from_millis(200),
            hello_timeout: Duration::from_millis(500),
            reconnect_delay: Duration::from_millis(50),
            initial_retry_delay: Duration::from_millis(20),
            max_reconnects: 2,
            initial_max_attempts: 4,
            poll_interval: Duration::from_millis(20),
        }
    }

    fn recv_until(
        client: &KanataClient,
        timeout: Duration,
        predicate: impl Fn(&ClientEvent) -> bool,
    ) -> Option<ClientEvent> {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if let Some(event) = client.poll_event() {
                if predicate(&event) {
                    return Some(event);
                }
            } else {
                thread::sleep(Duration::from_millis(10));
            }
        }
        None
    }

    #[test]
    fn connects_after_async_layer_change_and_parses_reload() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind");
        let address = listener.local_addr().expect("addr");
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().expect("accept");
            let mut reader = io::BufReader::new(stream.try_clone().expect("clone"));
            let mut writer = stream;
            let mut line = String::new();
            let _ = io::BufRead::read_line(&mut reader, &mut line);
            assert!(line.contains("Hello"));
            let _ = writer.write_all(
                b"{\"LayerChange\":{\"new\":\"base\"}}\n{\"HelloOk\":{\"version\":\"1.12.0\",\"protocol\":1}}\n",
            );
            line.clear();
            let _ = io::BufRead::read_line(&mut reader, &mut line);
            assert!(line.contains("Reload"));
            let _ = writer.write_all(b"{\"status\":\"Ok\"}\n");
            let _ = writer.write_all(b"{\"ConfigFileReload\":{\"new\":\"x.kbd\"}}\n");
            let _ = writer.write_all(b"{\"ReloadResult\":{\"ok\":true}}\n");
            thread::sleep(Duration::from_millis(200));
        });

        let client = KanataClient::start_with_options(address, test_options()).expect("client");
        assert!(recv_until(&client, Duration::from_secs(2), |event| {
            matches!(event, ClientEvent::Connected { .. })
        })
        .is_some());
        assert!(client.is_connected());

        client.reload(true, 5000).expect("send reload");
        assert!(recv_until(&client, Duration::from_secs(2), |event| {
            matches!(event, ClientEvent::ReloadResult { ok: true, .. })
        })
        .is_some());
        server.join().expect("server");
    }

    #[test]
    fn parses_message_push_string_and_array() {
        let (tx, rx) = mpsc::channel();
        dispatch_line(r#"{"MessagePush":{"message":"capsx:settings"}}"#, &tx);
        match rx.recv().expect("string message") {
            ClientEvent::MessagePush { message, .. } => {
                assert_eq!(message, "capsx:settings");
            }
            other => panic!("意外事件：{other:?}"),
        }

        dispatch_line(r#"{"MessagePush":{"message":["capsx:settings"]}}"#, &tx);
        match rx.recv().expect("array message") {
            ClientEvent::MessagePush { message, .. } => {
                assert_eq!(message, "capsx:settings");
            }
            other => panic!("意外事件：{other:?}"),
        }
    }

    #[test]
    fn malformed_line_does_not_kill_connection() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind");
        let address = listener.local_addr().expect("addr");
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().expect("accept");
            let mut reader = io::BufReader::new(stream.try_clone().expect("clone"));
            let mut writer = stream;
            let mut line = String::new();
            let _ = io::BufRead::read_line(&mut reader, &mut line);
            let _ = writer.write_all(b"not json\n");
            let _ = writer.write_all(b"{\"HelloOk\":{\"version\":\"1.12.0\"}}\n");
            thread::sleep(Duration::from_millis(300));
        });

        let client = KanataClient::start_with_options(address, test_options()).expect("client");
        assert!(recv_until(&client, Duration::from_secs(2), |event| {
            matches!(event, ClientEvent::ParseError { .. })
        })
        .is_some());
        assert!(recv_until(&client, Duration::from_secs(2), |event| {
            matches!(event, ClientEvent::Connected { .. })
        })
        .is_some());
        server.join().expect("server");
    }

    #[test]
    fn reconnects_after_disconnect() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind");
        let address = listener.local_addr().expect("addr");
        let server = thread::spawn(move || {
            for index in 0..2 {
                let (stream, _) = listener.accept().expect("accept");
                let mut reader = io::BufReader::new(stream.try_clone().expect("clone"));
                let mut writer = stream;
                let mut line = String::new();
                let _ = io::BufRead::read_line(&mut reader, &mut line);
                let _ = writer.write_all(b"{\"HelloOk\":{\"version\":\"1.12.0\"}}\n");
                if index == 0 {
                    let _ = writer.shutdown(std::net::Shutdown::Both);
                }
                thread::sleep(Duration::from_millis(200));
            }
        });

        let client = KanataClient::start_with_options(address, test_options()).expect("client");
        assert!(recv_until(&client, Duration::from_secs(2), |event| {
            matches!(event, ClientEvent::Connected { .. })
        })
        .is_some());
        assert!(recv_until(&client, Duration::from_secs(2), |event| {
            matches!(event, ClientEvent::Disconnected { .. })
        })
        .is_some());
        assert!(recv_until(&client, Duration::from_secs(3), |event| {
            matches!(event, ClientEvent::Reconnecting { .. })
        })
        .is_some());
        assert!(recv_until(&client, Duration::from_secs(3), |event| {
            matches!(event, ClientEvent::Connected { .. })
        })
        .is_some());
        server.join().expect("server");
    }
}
