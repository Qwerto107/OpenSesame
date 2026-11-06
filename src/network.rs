use crate::model::{KnockStep, Protocol, Server};
use crate::{i18n::Message, msg};
use socket2::{Domain, Socket, Type};
use std::{
    collections::HashMap,
    io,
    net::{IpAddr, SocketAddr},
    sync::{Arc, Mutex, Weak},
    time::{Duration, Instant},
};
use tokio::{
    net::{TcpStream, UdpSocket},
    sync::{Mutex as AsyncMutex, OwnedMutexGuard},
    time::{sleep, timeout},
};

#[cfg(target_os = "linux")]
mod raw_tcp;
#[cfg(all(windows, feature = "desktop"))]
mod windivert;
#[cfg(all(windows, feature = "desktop"))]
pub use windivert::export_source as export_windivert_source;

#[derive(Clone, Default)]
pub struct Client {
    targets: Arc<Mutex<HashMap<IpAddr, Weak<AsyncMutex<()>>>>>,
}

impl Client {
    async fn target_lock(&self, ip: IpAddr) -> OwnedMutexGuard<()> {
        let lock = {
            let mut targets = self
                .targets
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            targets.retain(|_, lock| lock.strong_count() > 0);
            let ip = ip.to_canonical();
            if let Some(lock) = targets.get(&ip).and_then(Weak::upgrade) {
                lock
            } else {
                let lock = Arc::new(AsyncMutex::new(()));
                targets.insert(ip, Arc::downgrade(&lock));
                lock
            }
        };
        lock.lock_owned().await
    }

    pub async fn execute(
        &self,
        server: &Server,
        progress: impl FnMut(usize),
    ) -> Result<(IpAddr, Duration), Message> {
        let host = &server.host;
        let steps = &server.open_sequence;
        let interval = Duration::from_millis(server.interval_ms);
        let limit = Duration::from_secs(server.sequence_timeout_secs);
        let first = steps.first().ok_or("The knock sequence cannot be empty.")?;
        let target = timeout(limit, resolve(host, first.port))
            .await
            .map_err(|_| "Hostname resolution timed out.")??;
        // 同一 IP 的所有配置共用锁，防止其他序列或业务检测包打断 knockd 的匹配。
        let _guard = self.target_lock(target.ip()).await;
        execute_sequence_with_flags(
            &target.ip().to_string(),
            steps,
            interval,
            limit,
            server.tcp_flags_mask(),
            progress,
        )
        .await
    }

    pub async fn probe(&self, server: &Server, pinned: Option<IpAddr>) -> ProbeResult {
        let ip = if let Some(ip) = pinned {
            ip
        } else {
            match timeout(
                Duration::from_secs(server.check_timeout_secs),
                resolve(&server.host, server.service_port),
            )
            .await
            {
                Ok(Ok(address)) => address.ip(),
                Ok(Err(detail)) => {
                    return ProbeResult {
                        state: PortState::Error,
                        detail,
                    };
                }
                Err(_) => {
                    return ProbeResult {
                        state: PortState::Error,
                        detail: "Hostname resolution timed out.".into(),
                    };
                }
            }
        };
        let _guard = self.target_lock(ip).await;
        probe(server, Some(ip)).await
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum PortState {
    #[default]
    Idle,
    Checking,
    Reachable,
    Refused,
    TimedOut,
    Unknown,
    Error,
}

impl PortState {
    pub fn short_label(&self, protocol: Protocol) -> &'static str {
        if *self == Self::Reachable {
            if protocol == Protocol::Tcp {
                "Reachable"
            } else {
                "Responded"
            }
        } else {
            self.label(protocol)
        }
    }
    pub fn kind(&self) -> i32 {
        match self {
            Self::Idle | Self::Unknown => 0,
            Self::Checking => 1,
            Self::Reachable => 2,
            Self::Refused | Self::TimedOut => 3,
            Self::Error => 4,
        }
    }
    pub fn label(&self, protocol: Protocol) -> &'static str {
        match self {
            Self::Idle => "Not checked",
            Self::Checking => "Checking",
            Self::Reachable if protocol == Protocol::Tcp => "TCP reachable",
            Self::Reachable => "UDP responded",
            Self::Refused => "Connection refused",
            Self::TimedOut => "Timed out",
            Self::Unknown => "Unknown",
            Self::Error => "Check error",
        }
    }
}

#[derive(Clone, Debug)]
pub struct ProbeResult {
    pub state: PortState,
    pub detail: Message,
}

pub async fn resolve(host: &str, port: u16) -> Result<SocketAddr, Message> {
    let host = host.trim().trim_start_matches('[').trim_end_matches(']');
    if let Ok(ip) = host.parse::<IpAddr>() {
        return Ok(SocketAddr::new(ip, port));
    }
    let addresses: Vec<_> = tokio::net::lookup_host((host, port))
        .await
        .map_err(|error| msg!("Hostname resolution failed: {error}", "error" => error.to_string()))?
        .collect();
    // 一个序列固定使用同一地址，避免域名轮询把连续步骤分发给不同主机。
    addresses
        .iter()
        .find(|address| address.is_ipv4())
        .or(addresses.first())
        .copied()
        .ok_or_else(|| "No usable address was found for this hostname.".into())
}

pub async fn execute_sequence(
    host: &str,
    steps: &[KnockStep],
    interval: Duration,
    limit: Duration,
    progress: impl FnMut(usize),
) -> Result<(IpAddr, Duration), Message> {
    execute_sequence_with_flags(host, steps, interval, limit, 0x02, progress).await
}

async fn execute_sequence_with_flags(
    host: &str,
    steps: &[KnockStep],
    interval: Duration,
    limit: Duration,
    flags: u8,
    mut progress: impl FnMut(usize),
) -> Result<(IpAddr, Duration), Message> {
    let started = Instant::now();
    let operation = async {
        let first = steps.first().ok_or("The knock sequence cannot be empty.")?;
        let destination = resolve(host, first.port).await?;
        let custom = flags != 0x02 && steps.iter().any(|step| step.protocol == Protocol::Tcp);
        // 发送任何步骤前检查原始套接字权限，避免权限不足时仅发送了部分 UDP 步骤。
        #[cfg(target_os = "linux")]
        let raw = custom
            .then(|| raw_tcp::Sender::new(destination))
            .transpose()?;
        #[cfg(all(windows, feature = "desktop"))]
        let raw = custom
            .then(|| windivert::Sender::new(destination))
            .transpose()?;
        #[cfg(not(any(target_os = "linux", all(windows, feature = "desktop"))))]
        if custom {
            return Err("Custom TCP flags are not supported on this platform.".into());
        }
        for (index, step) in steps.iter().enumerate() {
            let target = SocketAddr::new(destination.ip(), step.port);
            let socket = match step.protocol {
                Protocol::Tcp => {
                    #[cfg(any(target_os = "linux", all(windows, feature = "desktop")))]
                    if let Some(raw) = &raw {
                        raw.send(step.port, flags)?;
                        progress(index + 1);
                        if index + 1 < steps.len() {
                            sleep(interval).await;
                        }
                        continue;
                    }
                    Some(start_tcp_knock(target)?)
                }
                Protocol::Udp => {
                    let socket = UdpSocket::bind(bind_address(target))
                        .await
                        .map_err(network_error)?;
                    socket.send_to(&[], target).await.map_err(network_error)?;
                    None
                }
            };
            progress(index + 1);
            // 短暂保留 TCP 套接字让 SYN 发出，随后关闭，避免系统重传干扰敲门顺序。
            let hold = if index + 1 == steps.len() {
                Duration::from_millis(25)
            } else {
                interval.min(Duration::from_millis(25))
            };
            sleep(hold).await;
            drop(socket);
            if index + 1 < steps.len() {
                sleep(interval.saturating_sub(hold)).await;
            }
        }
        Ok((destination.ip(), started.elapsed()))
    };
    timeout(limit, operation).await.map_err(|_| {
        Message::from("Sequence timed out. Check the address, interval and timeout.")
    })?
}

fn start_tcp_knock(target: SocketAddr) -> Result<Socket, Message> {
    let socket = Socket::new(
        Domain::for_address(target),
        Type::STREAM,
        Some(socket2::Protocol::TCP),
    )
    .map_err(network_error)?;
    socket.set_nonblocking(true).map_err(network_error)?;
    match socket.connect(&target.into()) {
        Ok(()) => Ok(socket),
        Err(error)
            if error.kind() == io::ErrorKind::WouldBlock
                || error.kind() == io::ErrorKind::ConnectionRefused
                || connect_in_progress(&error) =>
        {
            Ok(socket)
        }
        Err(error) => Err(network_error(error)),
    }
}

fn connect_in_progress(error: &io::Error) -> bool {
    // 非阻塞连接已开始时仍算敲门发送成功，各系统的 EINPROGRESS 数值不同。
    #[cfg(unix)]
    let pending = matches!(
        error.raw_os_error(),
        Some(libc::EINPROGRESS | libc::EALREADY)
    );
    #[cfg(windows)]
    let pending = matches!(error.raw_os_error(), Some(10035..=10037));
    pending
}

pub async fn probe(server: &Server, pinned: Option<IpAddr>) -> ProbeResult {
    let operation = async {
        let target = match pinned {
            Some(ip) => SocketAddr::new(ip, server.service_port),
            None => resolve(&server.host, server.service_port)
                .await
                .map_err(|detail| ProbeResult {
                    state: PortState::Error,
                    detail,
                })?,
        };
        match server.service_protocol {
            Protocol::Tcp => {
                let connection = TcpStream::connect(target).await.map_err(tcp_error)?;
                drop(connection);
                Ok(ProbeResult {
                    state: PortState::Reachable,
                    detail: "A connection was established; authentication was not checked.".into(),
                })
            }
            Protocol::Udp => {
                let socket = UdpSocket::bind(bind_address(target))
                    .await
                    .map_err(udp_error)?;
                socket.connect(target).await.map_err(udp_error)?;
                socket.send(&[]).await.map_err(udp_error)?;
                let mut data = [0u8; 2048];
                let received = socket.recv(&mut data).await.map_err(udp_error)?;
                Ok(ProbeResult {
                    state: PortState::Reachable,
                    detail: msg!("The target replied with {bytes} bytes; authentication was not checked.", "bytes" => received),
                })
            }
        }
    };
    match timeout(Duration::from_secs(server.check_timeout_secs), operation).await {
        Ok(Ok(result)) | Ok(Err(result)) => result,
        Err(_) if server.service_protocol == Protocol::Udp => ProbeResult {
            state: PortState::Unknown,
            detail: "No response received; this does not mean the port is closed.".into(),
        },
        Err(_) => ProbeResult {
            state: PortState::TimedOut,
            detail: "No connection was established within the configured timeout.".into(),
        },
    }
}

fn bind_address(target: SocketAddr) -> SocketAddr {
    if target.is_ipv4() {
        "0.0.0.0:0".parse().unwrap()
    } else {
        "[::]:0".parse().unwrap()
    }
}

fn network_error(error: io::Error) -> Message {
    msg!("Network operation failed: {error}", "error" => error.to_string())
}

fn tcp_error(error: io::Error) -> ProbeResult {
    let state = match error.kind() {
        io::ErrorKind::ConnectionRefused => PortState::Refused,
        io::ErrorKind::TimedOut => PortState::TimedOut,
        _ => PortState::Error,
    };
    ProbeResult {
        state,
        detail: error.to_string().into(),
    }
}

fn udp_error(error: io::Error) -> ProbeResult {
    let state = match error.kind() {
        io::ErrorKind::ConnectionRefused | io::ErrorKind::ConnectionReset => PortState::Refused,
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock => PortState::Unknown,
        _ => PortState::Error,
    };
    ProbeResult {
        state,
        detail: error.to_string().into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn custom_flags_fail_before_udp_when_raw_sockets_are_unavailable() {
        // 普通测试不安装驱动；管理员环境的真实发包验证由显式集成测试完成。
        #[cfg(all(windows, feature = "desktop"))]
        if unsafe { windows_sys::Win32::UI::Shell::IsUserAnAdmin() } != 0 {
            return;
        }
        #[cfg(target_os = "linux")]
        if raw_tcp::Sender::new("127.0.0.1:7000".parse().unwrap()).is_ok() {
            return;
        }
        let udp = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let server = Server {
            host: "127.0.0.1".into(),
            tcp_flags: vec![crate::model::TcpFlag::Ack],
            open_sequence: vec![
                KnockStep {
                    port: udp.local_addr().unwrap().port(),
                    protocol: Protocol::Udp,
                },
                KnockStep {
                    port: 7000,
                    protocol: Protocol::Tcp,
                },
            ],
            ..Server::default()
        };
        let mut progress = vec![];
        assert!(
            Client::default()
                .execute(&server, |step| progress.push(step))
                .await
                .is_err()
        );
        assert!(progress.is_empty());
        assert!(
            timeout(Duration::from_millis(50), udp.recv_from(&mut [0; 1]))
                .await
                .is_err()
        );
    }

    fn local(port: u16, protocol: Protocol) -> Server {
        Server {
            name: "本地测试".into(),
            host: "127.0.0.1".into(),
            service_port: port,
            service_protocol: protocol,
            check_timeout_secs: 1,
            ..Server::default()
        }
    }
    #[tokio::test]
    async fn tcp_reachable_and_closed_port() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let server = local(listener.local_addr().unwrap().port(), Protocol::Tcp);
        assert_eq!(probe(&server, None).await.state, PortState::Reachable);
        drop(listener);
        // 本机防火墙可能丢弃拒绝响应；两种失败结果都不能误报端口可连接。
        assert!(matches!(
            probe(&server, None).await.state,
            PortState::Refused | PortState::TimedOut
        ));
    }
    #[tokio::test]
    async fn udp_response_and_silence_are_distinct() {
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let server = local(socket.local_addr().unwrap().port(), Protocol::Udp);
        let responder = tokio::spawn(async move {
            let mut bytes = [0; 8];
            let (_, peer) = socket.recv_from(&mut bytes).await.unwrap();
            socket.send_to(b"ok", peer).await.unwrap();
        });
        assert_eq!(probe(&server, None).await.state, PortState::Reachable);
        responder.await.unwrap();
        let silent = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        assert_eq!(
            probe(
                &local(silent.local_addr().unwrap().port(), Protocol::Udp),
                None
            )
            .await
            .state,
            PortState::Unknown
        );
    }
    #[tokio::test]
    async fn sends_udp_sequence_in_order() {
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let port = socket.local_addr().unwrap().port();
        let steps = vec![
            KnockStep {
                port,
                protocol: Protocol::Udp
            };
            3
        ];
        let mut progress = vec![];
        let receive = tokio::spawn(async move {
            let mut bytes = [0; 1];
            let mut times = vec![];
            for _ in 0..3 {
                socket.recv_from(&mut bytes).await.unwrap();
                times.push(Instant::now());
            }
            times
        });
        execute_sequence(
            "127.0.0.1",
            &steps,
            Duration::from_millis(50),
            Duration::from_secs(2),
            |step| progress.push(step),
        )
        .await
        .unwrap();
        let times = receive.await.unwrap();
        assert_eq!(progress, vec![1, 2, 3]);
        assert!(times[2].duration_since(times[0]) >= Duration::from_millis(80));
    }
    #[tokio::test]
    async fn sequence_enforces_deadline() {
        let steps = vec![
            KnockStep {
                port: 9,
                protocol: Protocol::Udp
            };
            3
        ];
        assert!(
            execute_sequence(
                "127.0.0.1",
                &steps,
                Duration::from_secs(1),
                Duration::from_millis(30),
                |_| {}
            )
            .await
            .is_err()
        );
    }
    #[tokio::test]
    async fn tcp_knock_does_not_wait_for_connection_success() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let steps = [KnockStep {
            port,
            protocol: Protocol::Tcp,
        }];
        assert!(
            execute_sequence(
                "127.0.0.1",
                &steps,
                Duration::from_millis(100),
                Duration::from_secs(1),
                |_| {}
            )
            .await
            .is_ok()
        );
    }

    #[tokio::test]
    async fn probing_cannot_interrupt_a_sequence_on_the_same_ip() {
        let udp = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let tcp = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let steps = vec![
            KnockStep {
                port: udp.local_addr().unwrap().port(),
                protocol: Protocol::Udp
            };
            2
        ];
        let client = Client::default();
        let sender = client.clone();
        let knock = tokio::spawn(async move {
            sender
                .execute(
                    &Server {
                        host: "127.0.0.1".into(),
                        open_sequence: steps,
                        interval_ms: 150,
                        sequence_timeout_secs: 2,
                        ..Server::default()
                    },
                    |_| {},
                )
                .await
        });
        let mut buffer = [0; 1];
        udp.recv_from(&mut buffer).await.unwrap();
        let server = local(tcp.local_addr().unwrap().port(), Protocol::Tcp);
        let check = tokio::spawn(async move { client.probe(&server, None).await });
        sleep(Duration::from_millis(40)).await;
        assert!(!check.is_finished(), "业务检测必须等完整敲门序列结束");
        knock.await.unwrap().unwrap();
        assert_eq!(check.await.unwrap().state, PortState::Reachable);
    }
}
