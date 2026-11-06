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
