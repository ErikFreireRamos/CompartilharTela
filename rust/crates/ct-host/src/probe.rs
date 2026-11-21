//! Teste de alcance: o host confere se outro participante recebe conexões de
//! fora nas portas de host (TCP 7880 e UDP 7882).

use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};
use std::time::Duration;

use ct_core::protocol::{PUBLIC_PORT, RTC_UDP_PORT};
use ct_net::ip::url_host;
use reqwest::Client;
use serde::Deserialize;
use socket2::{Domain, Protocol, Socket, Type};
use tokio::net::UdpSocket;
use tokio::task::JoinHandle;
use tokio::time::{sleep, timeout};

pub const UDP_PROBE: &str = "CT-PROBE ";
pub const UDP_PROBE_OK: &str = "CT-PROBE-OK ";

#[derive(Deserialize)]
struct ProbeReply {
    nonce: Option<String>,
    id: Option<String>,
}

async fn probe_tcp(client: &Client, ip: &str, identity: &str, nonce: &str) -> bool {
    let url = format!("http://{}:{PUBLIC_PORT}/probe?nonce={nonce}", url_host(ip));
    let Ok(res) = client
        .get(url)
        .timeout(Duration::from_millis(2500))
        .send()
        .await
    else {
        return false;
    };
    let Ok(body) = res.json::<ProbeReply>().await else {
        return false;
    };
    body.nonce.as_deref() == Some(nonce) && body.id.as_deref() == Some(identity)
}

async fn probe_udp(ip: &str, nonce: &str) -> bool {
    let is_v6 = ip.contains(':');
    let bind_addr = if is_v6 { "[::]:0" } else { "0.0.0.0:0" };
    let Ok(socket) = UdpSocket::bind(bind_addr).await else {
        return false;
    };
    let target = format!("{}:{RTC_UDP_PORT}", url_host(ip));
    let payload = format!("{UDP_PROBE}{nonce}");
    let expected = format!("{UDP_PROBE_OK}{nonce}");
    let mut buf = [0u8; 256];

    for _ in 0..4 {
        let _ = socket.send_to(payload.as_bytes(), &target).await;
        if let Ok(Ok(n)) = timeout(Duration::from_millis(500), socket.recv(&mut buf)).await
            && &buf[..n] == expected.as_bytes()
        {
            return true;
        }
    }
    false
}

pub async fn probe_candidate(ip: &str, identity: &str) -> bool {
    let client = Client::new();
    for attempt in 0..3 {
        let nonce_bytes: [u8; 8] = rand::random();
        let nonce: String = nonce_bytes.iter().map(|b| format!("{b:02x}")).collect();
        if probe_tcp(&client, ip, identity, &nonce).await && probe_udp(ip, &nonce).await {
            return true;
        }
        if attempt < 2 {
            sleep(Duration::from_secs(3)).await;
        }
    }
    false
}

/// Eco UDP na porta de mídia (7882) para candidatos; precisa ser fechado antes de virar host.
pub struct UdpProbeResponder {
    task: JoinHandle<()>,
}

impl UdpProbeResponder {
    pub fn start() -> Option<Self> {
        let udp = bind_udp_dual_stack(RTC_UDP_PORT).or_else(|| bind_udp_v4(RTC_UDP_PORT))?;
        let task = tokio::spawn(async move {
            let mut buf = [0u8; 256];
            loop {
                let Ok((n, peer)) = udp.recv_from(&mut buf).await else {
                    break;
                };
                if let Ok(text) = std::str::from_utf8(&buf[..n])
                    && let Some(nonce) = text.strip_prefix(UDP_PROBE)
                {
                    let reply = format!("{UDP_PROBE_OK}{nonce}");
                    let _ = udp.send_to(reply.as_bytes(), peer).await;
                }
            }
        });
        Some(Self { task })
    }

    pub fn stop(self) {
        self.task.abort();
    }
}

impl Drop for UdpProbeResponder {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn bind_udp_dual_stack(port: u16) -> Option<UdpSocket> {
    let sock = Socket::new(Domain::IPV6, Type::DGRAM, Some(Protocol::UDP)).ok()?;
    let _ = sock.set_only_v6(false);
    sock.set_nonblocking(true).ok()?;
    sock.bind(&SocketAddr::from(SocketAddrV6::new(Ipv6Addr::UNSPECIFIED, port, 0, 0)).into())
        .ok()?;
    let std_sock: std::net::UdpSocket = sock.into();
    UdpSocket::from_std(std_sock).ok()
}

fn bind_udp_v4(port: u16) -> Option<UdpSocket> {
    let sock = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP)).ok()?;
    sock.set_nonblocking(true).ok()?;
    sock.bind(&SocketAddr::from(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, port)).into())
        .ok()?;
    let std_sock: std::net::UdpSocket = sock.into();
    UdpSocket::from_std(std_sock).ok()
}
