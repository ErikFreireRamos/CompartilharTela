use std::time::{Duration, Instant};

use ct_core::protocol::{PUBLIC_PORT, RTC_TCP_PORT, RTC_UDP_PORT};

use crate::ip::{is_private_ipv4, lan_interfaces, stable_ipv6};
use crate::stun::{IpFamily, stun_public_ip};
use crate::upnp::{PortSpec, Protocol, UpnpClient};

const HOST_PORTS: &[PortSpec] = &[
    PortSpec { port: PUBLIC_PORT, protocol: Protocol::Tcp },
    PortSpec { port: RTC_TCP_PORT, protocol: Protocol::Tcp },
    PortSpec { port: RTC_UDP_PORT, protocol: Protocol::Udp },
];

#[derive(Debug, Clone)]
pub struct NetworkInfo {
    pub public_ip: Option<String>,
    pub direct: bool,
    pub cgnat: bool,
    pub ipv6: Option<String>,
    pub lan_ips: Vec<String>,
}

pub struct NetworkManager {
    upnp: UpnpClient,
    upnp_enabled: bool,
    cached: Option<(NetworkInfo, Instant)>,
}

impl Default for NetworkManager {
    fn default() -> Self {
        Self::new()
    }
}

impl NetworkManager {
    pub fn new() -> Self {
        Self {
            upnp: UpnpClient::new(),
            upnp_enabled: std::env::var("CT_NO_UPNP").as_deref() != Ok("1"),
            cached: None,
        }
    }

    pub async fn resolve(&mut self) -> NetworkInfo {
        if let Some((info, at)) = &self.cached
            && at.elapsed() < Duration::from_secs(60)
        {
            return info.clone();
        }

        let lan_ips: Vec<String> = lan_interfaces()
            .into_iter()
            .map(|i| i.address.to_string())
            .collect();

        let router_ip = if self.upnp_enabled {
            self.upnp.external_ip().await
        } else {
            None
        };
        let (stun_v4, stun_v6) = tokio::join!(
            stun_public_ip(IpFamily::V4, Duration::from_millis(1500)),
            stun_public_ip(IpFamily::V6, Duration::from_millis(1500)),
        );

        let ipv6 = stun_v6.map(|seen| stable_ipv6().unwrap_or(seen));

        let mut public_ip = stun_v4;
        if public_ip.is_none()
            && let Some(ref rip) = router_ip
            && let Ok(v4) = rip.parse()
            && !is_private_ipv4(v4)
        {
            public_ip = Some(rip.clone());
        }
        if let Some(ref pip) = public_ip
            && let Ok(v4) = pip.parse()
            && is_private_ipv4(v4)
        {
            public_ip = None;
        }

        let cgnat = match (&router_ip, &public_ip) {
            (Some(r), Some(p)) => r != p,
            _ => false,
        };
        let direct = public_ip
            .as_ref()
            .is_some_and(|p| lan_ips.iter().any(|l| l == p));

        let info = NetworkInfo {
            public_ip,
            direct,
            cgnat,
            ipv6,
            lan_ips,
        };
        self.cached = Some((info.clone(), Instant::now()));
        info
    }

    pub async fn open_ports(&mut self) -> bool {
        let info = self.resolve().await;
        if info.direct {
            return true;
        }
        if info.cgnat {
            return false;
        }
        self.upnp_enabled && self.upnp.map_ports(HOST_PORTS).await
    }

    pub async fn close_ports(&mut self) {
        self.upnp.unmap_all().await;
    }
}
