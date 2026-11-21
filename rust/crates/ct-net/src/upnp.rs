//! Cliente UPnP IGD mínimo: SSDP discovery + quick-xml + SOAP port mapping.

use std::collections::HashSet;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use quick_xml::Reader;
use quick_xml::events::Event;
use reqwest::Client;
use tokio::net::UdpSocket;
use tokio::task::JoinHandle;
use tokio::time::{interval, timeout};
use url::Url;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    Tcp,
    Udp,
}

impl Protocol {
    pub fn as_str(self) -> &'static str {
        match self {
            Protocol::Tcp => "TCP",
            Protocol::Udp => "UDP",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PortSpec {
    pub port: u16,
    pub protocol: Protocol,
}

#[derive(Debug, Clone)]
struct Gateway {
    control_url: String,
    service_type: String,
    local_address: String,
}

const SSDP_ADDR: &str = "239.255.255.250:1900";
const SEARCH_TARGETS: &[&str] = &[
    "urn:schemas-upnp-org:device:InternetGatewayDevice:1",
    "urn:schemas-upnp-org:device:InternetGatewayDevice:2",
    "urn:schemas-upnp-org:service:WANIPConnection:1",
    "urn:schemas-upnp-org:service:WANIPConnection:2",
    "urn:schemas-upnp-org:service:WANPPPConnection:1",
];
const DESCRIPTION: &str = "CompartilhadorDeTela";
const RENEW_LEASE_SECONDS: u32 = 3600;

async fn ssdp_search(timeout_dur: Duration) -> Vec<String> {
    let Ok(socket) = UdpSocket::bind("0.0.0.0:0").await else {
        return Vec::new();
    };
    let Ok(dest) = SSDP_ADDR.parse::<SocketAddr>() else {
        return Vec::new();
    };
    for st in SEARCH_TARGETS {
        let req = format!(
            "M-SEARCH * HTTP/1.1\r\nHOST: {SSDP_ADDR}\r\nMAN: \"ssdp:discover\"\r\nMX: 2\r\nST: {st}\r\n\r\n"
        );
        let _ = socket.send_to(req.as_bytes(), dest).await;
    }

    let mut locations = HashSet::new();
    let deadline = Instant::now() + timeout_dur;
    let mut grace_deadline: Option<Instant> = None;
    let mut buf = [0u8; 2048];

    loop {
        let now = Instant::now();
        let target_deadline = grace_deadline.map_or(deadline, |g| g.min(deadline));
        if now >= target_deadline {
            break;
        }
        let rem = target_deadline.saturating_duration_since(now);
        match timeout(rem, socket.recv(&mut buf)).await {
            Ok(Ok(n)) => {
                let text = String::from_utf8_lossy(&buf[..n]);
                for line in text.lines() {
                    if let Some(rest) = line
                        .strip_prefix("LOCATION:")
                        .or_else(|| line.strip_prefix("location:"))
                        .or_else(|| line.strip_prefix("Location:"))
                    {
                        let loc = rest.trim();
                        if !loc.is_empty() {
                            locations.insert(loc.to_string());
                            grace_deadline.get_or_insert_with(|| Instant::now() + Duration::from_millis(400));
                        }
                    }
                }
            }
            _ => break,
        }
    }
    locations.into_iter().collect()
}

fn xml_tag_value(xml: &str, target_tag: &str) -> Option<String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut in_tag = false;
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = String::from_utf8_lossy(e.local_name().as_ref()).to_string();
                if name.eq_ignore_ascii_case(target_tag) {
                    in_tag = true;
                }
            }
            Ok(Event::Text(t)) if in_tag => {
                return Some(t.unescape().ok()?.trim().to_string());
            }
            Ok(Event::End(_)) => in_tag = false,
            Ok(Event::Eof) | Err(_) => return None,
            _ => {}
        }
    }
}

fn parse_gateways(xml: &str, location: &str, local_address: &str) -> Vec<Gateway> {
    let base_str = xml_tag_value(xml, "URLBase").unwrap_or_else(|| location.to_string());
    let Ok(base_url) = Url::parse(&base_str) else {
        return Vec::new();
    };

    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut in_service = false;
    let mut cur_tag = String::new();
    let mut cur_service_type: Option<String> = None;
    let mut cur_control_url: Option<String> = None;
    let mut gateways = Vec::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = String::from_utf8_lossy(e.local_name().as_ref()).to_string();
                if name.eq_ignore_ascii_case("service") {
                    in_service = true;
                    cur_service_type = None;
                    cur_control_url = None;
                } else if in_service {
                    cur_tag = name;
                }
            }
            Ok(Event::Text(t)) if in_service => {
                if let Ok(text) = t.unescape() {
                    let val = text.trim().to_string();
                    if cur_tag.eq_ignore_ascii_case("serviceType") {
                        cur_service_type = Some(val);
                    } else if cur_tag.eq_ignore_ascii_case("controlURL") {
                        cur_control_url = Some(val);
                    }
                }
            }
            Ok(Event::End(e)) => {
                let name = String::from_utf8_lossy(e.local_name().as_ref()).to_string();
                if name.eq_ignore_ascii_case("service") {
                    in_service = false;
                    if let (Some(st), Some(cu)) = (cur_service_type.take(), cur_control_url.take())
                        && (st.contains("WANIPConnection:") || st.contains("WANPPPConnection:"))
                        && let Ok(resolved) = base_url.join(&cu)
                    {
                        gateways.push(Gateway {
                            service_type: st,
                            control_url: resolved.to_string(),
                            local_address: local_address.to_string(),
                        });
                    }
                }
                cur_tag.clear();
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }

    gateways.sort_by_key(|g| !g.service_type.contains("WANIP"));
    gateways
}

async fn soap_call(
    client: &Client,
    gw: &Gateway,
    action: &str,
    args: &[(&str, String)],
) -> Result<String, String> {
    let args_xml: String = args
        .iter()
        .map(|(k, v)| format!("<{k}>{v}</{k}>"))
        .collect();
    let body = format!(
        "<?xml version=\"1.0\"?>\
         <s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" \
         s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\">\
         <s:Body><u:{action} xmlns:u=\"{}\">{args_xml}</u:{action}></s:Body></s:Envelope>",
        gw.service_type
    );
    let res = client
        .post(&gw.control_url)
        .header("Content-Type", "text/xml; charset=\"utf-8\"")
        .header("SOAPAction", format!("\"{}#{action}\"", gw.service_type))
        .timeout(Duration::from_secs(3))
        .body(body)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = res.status();
    let text = res.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!("{action} falhou ({status})"));
    }
    Ok(text)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MapOutcome {
    Permanent,
    Lease,
    Failed,
}

pub struct UpnpClient {
    http: Client,
    gateway: Option<Gateway>,
    external_address: Option<String>,
    last_attempt: Option<Instant>,
    mapped: Vec<PortSpec>,
    renew_task: Option<JoinHandle<()>>,
}

impl Default for UpnpClient {
    fn default() -> Self {
        Self::new()
    }
}

impl UpnpClient {
    pub fn new() -> Self {
        Self {
            http: Client::builder()
                .timeout(Duration::from_secs(3))
                .build()
                .unwrap_or_default(),
            gateway: None,
            external_address: None,
            last_attempt: None,
            mapped: Vec::new(),
            renew_task: None,
        }
    }

    pub async fn discover(&mut self) -> bool {
        if self.gateway.is_some() {
            return true;
        }
        if let Some(last) = self.last_attempt
            && last.elapsed() < Duration::from_secs(30)
        {
            return false;
        }
        self.last_attempt = Some(Instant::now());

        let local_ip = crate::ip::lan_interfaces()
            .first()
            .map(|i| i.address.to_string())
            .unwrap_or_else(|| "127.0.0.1".to_string());

        for location in ssdp_search(Duration::from_millis(2500)).await {
            let Ok(res) = self.http.get(&location).send().await else {
                continue;
            };
            let Ok(body) = res.text().await else {
                continue;
            };
            for gw in parse_gateways(&body, &location, &local_ip) {
                if let Ok(soap_res) = soap_call(&self.http, &gw, "GetExternalIPAddress", &[]).await
                    && let Some(ip) = xml_tag_value(&soap_res, "NewExternalIPAddress")
                    && ip.parse::<std::net::Ipv4Addr>().is_ok()
                    && ip != "0.0.0.0"
                {
                    self.gateway = Some(gw);
                    self.external_address = Some(ip);
                    return true;
                }
            }
        }
        false
    }

    pub async fn external_ip(&mut self) -> Option<String> {
        if self.discover().await {
            self.external_address.clone()
        } else {
            None
        }
    }

    pub async fn map_ports(&mut self, ports: &[PortSpec]) -> bool {
        if !self.discover().await {
            return false;
        }
        let Some(gw) = self.gateway.clone() else {
            return false;
        };
        let mut all_ok = true;
        let mut needs_renew = false;

        for &spec in ports {
            match Self::map_one(&self.http, &gw, spec).await {
                MapOutcome::Failed => all_ok = false,
                outcome => {
                    if outcome == MapOutcome::Lease {
                        needs_renew = true;
                    }
                    if !self.mapped.contains(&spec) {
                        self.mapped.push(spec);
                    }
                }
            }
        }

        if needs_renew && self.renew_task.is_none() {
            let http = self.http.clone();
            let gw_clone = gw.clone();
            let mapped = self.mapped.clone();
            self.renew_task = Some(tokio::spawn(async move {
                let mut ticker = interval(Duration::from_secs(u64::from(RENEW_LEASE_SECONDS - 600)));
                ticker.tick().await;
                loop {
                    ticker.tick().await;
                    for &spec in &mapped {
                        let _ = Self::map_one(&http, &gw_clone, spec).await;
                    }
                }
            }));
        }
        all_ok
    }

    async fn map_one(http: &Client, gw: &Gateway, spec: PortSpec) -> MapOutcome {
        let make_args = |lease: u32| {
            vec![
                ("NewRemoteHost", String::new()),
                ("NewExternalPort", spec.port.to_string()),
                ("NewProtocol", spec.protocol.as_str().to_string()),
                ("NewInternalPort", spec.port.to_string()),
                ("NewInternalClient", gw.local_address.clone()),
                ("NewEnabled", "1".to_string()),
                ("NewPortMappingDescription", DESCRIPTION.to_string()),
                ("NewLeaseDuration", lease.to_string()),
            ]
        };

        if soap_call(http, gw, "AddPortMapping", &make_args(0)).await.is_ok() {
            return MapOutcome::Permanent;
        }
        if soap_call(http, gw, "AddPortMapping", &make_args(RENEW_LEASE_SECONDS))
            .await
            .is_ok()
        {
            return MapOutcome::Lease;
        }
        let query_args = vec![
            ("NewRemoteHost", String::new()),
            ("NewExternalPort", spec.port.to_string()),
            ("NewProtocol", spec.protocol.as_str().to_string()),
        ];
        if let Ok(entry) = soap_call(http, gw, "GetSpecificPortMappingEntry", &query_args).await
            && xml_tag_value(&entry, "NewInternalClient").as_deref() == Some(gw.local_address.as_str())
        {
            return MapOutcome::Permanent;
        }
        MapOutcome::Failed
    }

    pub async fn unmap_all(&mut self) {
        if let Some(handle) = self.renew_task.take() {
            handle.abort();
        }
        let Some(gw) = self.gateway.clone() else {
            self.mapped.clear();
            return;
        };
        let mapped = std::mem::take(&mut self.mapped);
        for spec in mapped {
            let args = vec![
                ("NewRemoteHost", String::new()),
                ("NewExternalPort", spec.port.to_string()),
                ("NewProtocol", spec.protocol.as_str().to_string()),
            ];
            let _ = soap_call(&self.http, &gw, "DeletePortMapping", &args).await;
        }
    }
}
