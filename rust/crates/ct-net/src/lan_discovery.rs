//! Acha o host na mesma rede local via UDP broadcast na porta 7883.

use std::collections::HashSet;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::time::Duration;

use ct_core::protocol::DISCOVERY_PORT;
use socket2::{Domain, Protocol, Socket, Type};
use tokio::net::UdpSocket;
use tokio::task::JoinHandle;
use tokio::time::timeout;

use crate::ip::lan_interfaces;

const FIND_PREFIX: &str = "CT-FIND ";
const HERE_PREFIX: &str = "CT-HERE ";

pub struct LanResponder {
    task: JoinHandle<()>,
}

impl LanResponder {
    pub async fn start(secret: u32) -> Option<Self> {
        let sock = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP)).ok()?;
        let _ = sock.set_reuse_address(true);
        sock.set_nonblocking(true).ok()?;
        sock.bind(&SocketAddr::from(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, DISCOVERY_PORT)).into())
            .ok()?;
        let std_sock: std::net::UdpSocket = sock.into();
        let udp = UdpSocket::from_std(std_sock).ok()?;

        let expected = format!("{FIND_PREFIX}{secret}");
        let reply = format!("{HERE_PREFIX}{secret}");
        let task = tokio::spawn(async move {
            let mut buf = [0u8; 256];
            loop {
                let Ok((n, peer)) = udp.recv_from(&mut buf).await else {
                    break;
                };
                if buf[..n] == *expected.as_bytes() {
                    let _ = udp.send_to(reply.as_bytes(), peer).await;
                }
            }
        });
        Some(Self { task })
    }

    pub fn close(self) {
        self.task.abort();
    }
}

impl Drop for LanResponder {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Devolve o IP local do host da sala, ou `None` se não estiver na mesma rede.
/// `CT_NO_LAN_DISCOVERY=1` desliga a busca.
pub async fn find_on_lan(secret: u32, timeout_dur: Duration) -> Option<String> {
    if std::env::var("CT_NO_LAN_DISCOVERY").as_deref() == Ok("1") {
        return None;
    }
    let udp = UdpSocket::bind("0.0.0.0:0").await.ok()?;
    let _ = udp.set_broadcast(true);

    let mut targets = HashSet::new();
    targets.insert(Ipv4Addr::LOCALHOST);
    targets.insert(Ipv4Addr::BROADCAST);
    for iface in lan_interfaces() {
        targets.insert(iface.broadcast);
    }

    let query = format!("{FIND_PREFIX}{secret}");
    for ip in targets {
        let _ = udp.send_to(query.as_bytes(), (ip, DISCOVERY_PORT)).await;
    }

    let expected = format!("{HERE_PREFIX}{secret}");
    let mut buf = [0u8; 256];
    let res = timeout(timeout_dur, async {
        loop {
            let (n, peer) = udp.recv_from(&mut buf).await.ok()?;
            if buf[..n] == *expected.as_bytes() {
                return Some(peer.ip().to_string());
            }
        }
    })
    .await;
    res.ok().flatten()
}
