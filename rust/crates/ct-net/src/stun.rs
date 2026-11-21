//! Descobre o IP público com uma requisição STUN (RFC 5389), pelo IPv4 ou IPv6.

use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;

use tokio::net::{UdpSocket, lookup_host};
use tokio::time::timeout;

const MAGIC_COOKIE: u32 = 0x2112_a442;
const SERVERS: &[(&str, u16)] = &[
    ("stun.l.google.com", 19302),
    ("stun.cloudflare.com", 3478),
    ("stun1.l.google.com", 19302),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IpFamily {
    V4,
    V6,
}

pub(crate) fn parse_mapped_address(msg: &[u8], transaction_id: &[u8; 12]) -> Option<String> {
    if msg.len() < 20 {
        return None;
    }
    let body_len = u16::from_be_bytes([msg[2], msg[3]]) as usize;
    let end = msg.len().min(20 + body_len);
    let mut offset = 20;

    while offset + 4 <= end {
        let attr_type = u16::from_be_bytes([msg[offset], msg[offset + 1]]);
        let attr_len = u16::from_be_bytes([msg[offset + 2], msg[offset + 3]]) as usize;
        let value = offset + 4;
        if value + attr_len > end {
            break;
        }
        let is_xor = attr_type == 0x0020;
        if (is_xor || attr_type == 0x0001) && attr_len >= 8 {
            let family = msg[value + 1];
            if family == 0x01 && attr_len >= 8 {
                let raw = u32::from_be_bytes([
                    msg[value + 4],
                    msg[value + 5],
                    msg[value + 6],
                    msg[value + 7],
                ]);
                let ip = if is_xor { raw ^ MAGIC_COOKIE } else { raw };
                return Some(Ipv4Addr::from(ip).to_string());
            }
            if family == 0x02 && attr_len >= 20 {
                let mut bytes = [0u8; 16];
                bytes.copy_from_slice(&msg[value + 4..value + 20]);
                if is_xor {
                    let cookie_bytes = MAGIC_COOKIE.to_be_bytes();
                    for i in 0..4 {
                        bytes[i] ^= cookie_bytes[i];
                    }
                    for i in 0..12 {
                        bytes[4 + i] ^= transaction_id[i];
                    }
                }
                return Some(Ipv6Addr::from(bytes).to_string());
            }
        }
        let pad = (4 - (attr_len % 4)) % 4;
        offset = value + attr_len + pad;
    }
    None
}

async fn query_server(
    host: &str,
    port: u16,
    family: IpFamily,
    timeout_dur: Duration,
) -> Option<String> {
    let addrs = timeout(timeout_dur, lookup_host((host, port))).await.ok()?.ok()?;
    let target: SocketAddr = addrs.into_iter().find(|a| match family {
        IpFamily::V4 => a.is_ipv4(),
        IpFamily::V6 => a.is_ipv6(),
    })?;

    let bind_addr = match family {
        IpFamily::V4 => "0.0.0.0:0",
        IpFamily::V6 => "[::]:0",
    };
    let socket = UdpSocket::bind(bind_addr).await.ok()?;

    let transaction_id: [u8; 12] = rand::random();
    let mut request = [0u8; 20];
    request[0..2].copy_from_slice(&0x0001u16.to_be_bytes());
    request[2..4].copy_from_slice(&0u16.to_be_bytes());
    request[4..8].copy_from_slice(&MAGIC_COOKIE.to_be_bytes());
    request[8..20].copy_from_slice(&transaction_id);

    socket.send_to(&request, target).await.ok()?;

    let mut buf = [0u8; 512];
    let len = timeout(timeout_dur, socket.recv(&mut buf)).await.ok()?.ok()?;
    let msg = &buf[..len];
    if msg.len() < 20 || u16::from_be_bytes([msg[0], msg[1]]) != 0x0101 {
        return None;
    }
    if &msg[8..20] != transaction_id {
        return None;
    }
    parse_mapped_address(msg, &transaction_id)
}

/// IP com que a internet vê este PC. Com `IpFamily::V6`, também confirma que o IPv6 funciona.
pub async fn stun_public_ip(family: IpFamily, timeout_dur: Duration) -> Option<String> {
    for &(host, port) in SERVERS {
        if let Some(ip) = query_server(host, port, family, timeout_dur).await {
            return Some(ip);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_xor_mapped_ipv4() {
        let tx_id = [1u8; 12];
        let ip = Ipv4Addr::new(189, 45, 7, 200);
        let xored = u32::from(ip) ^ MAGIC_COOKIE;
        let mut msg = vec![0u8; 32];
        msg[0..2].copy_from_slice(&0x0101u16.to_be_bytes());
        msg[2..4].copy_from_slice(&12u16.to_be_bytes());
        msg[4..8].copy_from_slice(&MAGIC_COOKIE.to_be_bytes());
        msg[8..20].copy_from_slice(&tx_id);
        // Attr: XOR-MAPPED-ADDRESS (0x0020), len 8
        msg[20..22].copy_from_slice(&0x0020u16.to_be_bytes());
        msg[22..24].copy_from_slice(&8u16.to_be_bytes());
        msg[25] = 0x01; // IPv4
        msg[28..32].copy_from_slice(&xored.to_be_bytes());

        assert_eq!(parse_mapped_address(&msg, &tx_id).as_deref(), Some("189.45.7.200"));
    }
}
