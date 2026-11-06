use super::network_error;
use crate::i18n::Message;
use socket2::{Domain, Socket, Type};
use std::{
    io,
    net::{IpAddr, SocketAddr},
};

pub(super) struct Sender {
    socket: Socket,
    // 保留源端口，防止并发任务或内核连接在发送期间复用它。
    source: Socket,
    destination: IpAddr,
}

impl Sender {
    pub(super) fn new(destination: SocketAddr) -> Result<Self, Message> {
        let socket = Socket::new(
            Domain::for_address(destination),
            Type::from(libc::SOCK_RAW),
            Some(socket2::Protocol::TCP),
        )
        .map_err(|error| {
            if error.kind() == io::ErrorKind::PermissionDenied {
                Message::from("Custom TCP flags require CAP_NET_RAW permission on the server.")
            } else {
                network_error(error)
            }
        })?;
        // 原始套接字的端口由 TCP 报文头指定；IPv6 的 sockaddr 端口必须为零。
        socket
            .connect(&SocketAddr::new(destination.ip(), 0).into())
            .map_err(network_error)?;
        socket.set_nonblocking(true).map_err(network_error)?;
        let local = socket
            .local_addr()
            .map_err(network_error)?
            .as_socket()
            .ok_or("Could not determine the TCP source address.")?;
        let source = Socket::new(
            Domain::for_address(local),
            Type::STREAM,
            Some(socket2::Protocol::TCP),
        )
        .map_err(network_error)?;
        source
            .bind(&SocketAddr::new(local.ip(), 0).into())
            .map_err(network_error)?;
        Ok(Self {
            socket,
            source,
            destination: destination.ip(),
        })
    }

    pub(super) fn send(&self, port: u16, flags: u8) -> Result<(), Message> {
        let local = self
            .source
            .local_addr()
            .map_err(network_error)?
            .as_socket()
            .ok_or("Could not determine the TCP source address.")?;
        let packet = packet(local, SocketAddr::new(self.destination, port), flags);
        let sent = self.socket.send(&packet).map_err(network_error)?;
        if sent != packet.len() {
            return Err("The TCP packet was not fully sent.".into());
        }
        Ok(())
    }
}

fn packet(source: SocketAddr, destination: SocketAddr, flags: u8) -> [u8; 20] {
    let mut tcp = [0u8; 20];
    tcp[..2].copy_from_slice(&source.port().to_be_bytes());
    tcp[2..4].copy_from_slice(&destination.port().to_be_bytes());
    tcp[4..8].copy_from_slice(&1u32.to_be_bytes());
    if flags & 0x10 != 0 {
        tcp[8..12].copy_from_slice(&1u32.to_be_bytes());
    }
    tcp[12] = 5 << 4;
    tcp[13] = flags;
    tcp[14..16].copy_from_slice(&1024u16.to_be_bytes());
    if flags & 0x20 != 0 {
        tcp[18..20].copy_from_slice(&1u16.to_be_bytes());
    }
    // Linux 生成 IP 头；TCP 校验和包含 IPv4/IPv6 伪首部，必须由发送方计算。
    let mut pseudo = Vec::with_capacity(60);
    match (source.ip(), destination.ip()) {
        (IpAddr::V4(src), IpAddr::V4(dst)) => {
            pseudo.extend(src.octets());
            pseudo.extend(dst.octets());
            pseudo.extend([0, 6, 0, 20]);
        }
        (IpAddr::V6(src), IpAddr::V6(dst)) => {
            pseudo.extend(src.octets());
            pseudo.extend(dst.octets());
            pseudo.extend([0, 0, 0, 20, 0, 0, 0, 6]);
        }
        _ => unreachable!("source and destination address families must match"),
    }
    pseudo.extend(tcp);
    tcp[16..18].copy_from_slice(&checksum(&pseudo).to_be_bytes());
    tcp
}

fn checksum(bytes: &[u8]) -> u16 {
    let mut sum: u32 = bytes
        .chunks(2)
        .map(|pair| u16::from_be_bytes([pair[0], *pair.get(1).unwrap_or(&0)]) as u32)
        .sum();
    while sum >> 16 != 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "requires CAP_NET_RAW; sends only to loopback"]
    async fn captures_custom_flags_on_ipv4_and_ipv6_loopback() {
        use crate::{
            model::{KnockStep, Protocol, Server, TcpFlag},
            network::Client,
        };
        use std::{io::Read, time::Duration};
        for host in ["127.0.0.1", "::1"] {
            for flags in [
                vec![TcpFlag::Syn, TcpFlag::Ack],
                vec![TcpFlag::Fin],
                vec![TcpFlag::Fin, TcpFlag::Psh, TcpFlag::Urg],
                vec![],
            ] {
                let ip: IpAddr = host.parse().unwrap();
                let capture = Socket::new(
                    Domain::for_address(SocketAddr::new(ip, 0)),
                    Type::from(libc::SOCK_RAW),
                    Some(socket2::Protocol::TCP),
                )
                .unwrap();
                capture.bind(&SocketAddr::new(ip, 0).into()).unwrap();
                capture
                    .set_read_timeout(Some(Duration::from_secs(1)))
                    .unwrap();
                let target = std::net::TcpListener::bind((ip, 0)).unwrap();
                let port = target.local_addr().unwrap().port();
                let udp = tokio::net::UdpSocket::bind((ip, 0)).await.unwrap();
                let server = Server {
                    host: host.into(),
                    tcp_flags: flags,
                    open_sequence: vec![
                        KnockStep {
                            port,
                            protocol: Protocol::Tcp,
                        },
                        KnockStep {
                            port: udp.local_addr().unwrap().port(),
                            protocol: Protocol::Udp,
                        },
                    ],
                    ..Server::default()
                };
                let mut progress = vec![];
                Client::default()
                    .execute(&server, |step| progress.push(step))
                    .await
                    .unwrap();
                assert_eq!(progress, vec![1, 2]);
                tokio::time::timeout(Duration::from_secs(1), udp.recv_from(&mut [0; 1]))
                    .await
                    .unwrap()
                    .unwrap();
                let mut bytes = [0; 2048];
                loop {
                    let len = (&capture).read(&mut bytes).unwrap();
                    let offset = if ip.is_ipv4() {
                        (bytes[0] & 0x0f) as usize * 4
                    } else {
                        0
                    };
                    let tcp = &bytes[offset..len];
                    if tcp.len() < 20 || tcp[2..4] != port.to_be_bytes() {
                        continue;
                    }
                    assert_eq!(tcp[13], server.tcp_flags_mask());
                    let source = SocketAddr::new(ip, u16::from_be_bytes([tcp[0], tcp[1]]));
                    assert_eq!(
                        tcp,
                        packet(source, SocketAddr::new(ip, port), server.tcp_flags_mask())
                    );
                    break;
                }
            }
        }
    }

    #[test]
    fn flags_and_checksums_match_ipv4_and_ipv6_wire_format() {
        for (src, dst, expected) in [
            ("192.0.2.1:40000", "198.51.100.2:7000", 0x12),
            ("[2001:db8::1]:40000", "[2001:db8::2]:7000", 0x29),
        ] {
            let src: SocketAddr = src.parse().unwrap();
            let dst: SocketAddr = dst.parse().unwrap();
            let tcp = packet(src, dst, expected);
            assert_eq!(&tcp[..4], &[0x9c, 0x40, 0x1b, 0x58]);
            assert_eq!(tcp[12], 0x50);
            assert_eq!(tcp[13], expected);
            let mut pseudo = Vec::new();
            match (src.ip(), dst.ip()) {
                (IpAddr::V4(a), IpAddr::V4(b)) => {
                    pseudo.extend(a.octets());
                    pseudo.extend(b.octets());
                    pseudo.extend([0, 6, 0, 20]);
                }
                (IpAddr::V6(a), IpAddr::V6(b)) => {
                    pseudo.extend(a.octets());
                    pseudo.extend(b.octets());
                    pseudo.extend([0, 0, 0, 20, 0, 0, 0, 6]);
                }
                _ => unreachable!(),
            }
            pseudo.extend(tcp);
            assert_eq!(checksum(&pseudo), 0);
            assert_eq!(tcp[11], u8::from(expected & 0x10 != 0));
            assert_eq!(tcp[19], u8::from(expected & 0x20 != 0));
        }
        assert_eq!(
            packet(
                "127.0.0.1:40000".parse().unwrap(),
                "127.0.0.1:7000".parse().unwrap(),
                0
            )[13],
            0
        );
    }
}
