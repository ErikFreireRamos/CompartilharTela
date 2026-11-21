use std::net::{IpAddr, Ipv4Addr};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanInterface {
    pub address: Ipv4Addr,
    pub broadcast: Ipv4Addr,
}

/// Faixas que não são alcançáveis pela internet (inclui CGNAT 100.64/10).
pub fn is_private_ipv4(ip: Ipv4Addr) -> bool {
    let [a, b, _, _] = ip.octets();
    a == 10
        || a == 127
        || (a == 172 && (16..=31).contains(&b))
        || (a == 192 && b == 168)
        || (a == 169 && b == 254)
        || (a == 100 && (64..=127).contains(&b))
}

/// IPv4 ou IPv6 que não é alcançável pela internet (local, link-local, ULA, CGNAT).
pub fn is_private_ip(ip: &str) -> bool {
    let Ok(addr) = ip.parse::<IpAddr>() else {
        return true;
    };
    match addr {
        IpAddr::V4(v4) => is_private_ipv4(v4),
        IpAddr::V6(v6) => {
            if let Some(mapped) = v6.to_ipv4_mapped() {
                return is_private_ipv4(mapped);
            }
            let seg0 = v6.segments()[0];
            v6.is_loopback()
                || (seg0 & 0xffc0) == 0xfe80 // link-local fe80::/10
                || (seg0 & 0xfe00) == 0xfc00 // ULA fc00::/7
        }
    }
}

/// IP pronto para URL: IPv6 vai entre colchetes.
pub fn url_host(ip: &str) -> String {
    if ip.contains(':') {
        format!("[{ip}]")
    } else {
        ip.to_string()
    }
}

/// "::ffff:1.2.3.4" -> "1.2.3.4", "::1" -> "127.0.0.1"
pub fn normalize_remote_ip(ip: IpAddr) -> String {
    match ip {
        IpAddr::V4(v4) => v4.to_string(),
        IpAddr::V6(v6) => {
            if let Some(mapped) = v6.to_ipv4_mapped() {
                mapped.to_string()
            } else if v6.is_loopback() {
                "127.0.0.1".to_string()
            } else {
                v6.to_string()
            }
        }
    }
}

/// Rede doméstica comum primeiro; VPNs (Radmin 26.x, Hamachi 25.x...) por último.
fn lan_priority(ip: Ipv4Addr) -> u8 {
    let [a, b, _, _] = ip.octets();
    if a == 192 && b == 168 {
        0
    } else if a == 10 {
        1
    } else if is_private_ipv4(ip) {
        2
    } else {
        3
    }
}

pub fn lan_interfaces() -> Vec<LanInterface> {
    let mut result = Vec::new();
    if let Ok(ifaces) = if_addrs::get_if_addrs() {
        for iface in ifaces {
            if iface.is_loopback() {
                continue;
            }
            if let if_addrs::IfAddr::V4(v4) = iface.addr {
                let ip_u32 = u32::from(v4.ip);
                let mask_u32 = u32::from(v4.netmask);
                let bcast = Ipv4Addr::from((ip_u32 & mask_u32) | !mask_u32);
                result.push(LanInterface {
                    address: v4.ip,
                    broadcast: bcast,
                });
            }
        }
    }
    result.sort_by_key(|i| lan_priority(i.address));
    result
}

/// Endereço IPv6 global estável (não temporário / aleatório).
#[cfg(windows)]
pub fn stable_ipv6() -> Option<String> {
    use std::net::Ipv6Addr;
    use windows::Win32::Foundation::ERROR_BUFFER_OVERFLOW;
    use windows::Win32::NetworkManagement::IpHelper::{
        GAA_FLAG_INCLUDE_PREFIX, GetAdaptersAddresses, IP_ADAPTER_ADDRESSES_LH,
    };
    use windows::Win32::Networking::WinSock::{
        AF_INET6, IpDadStatePreferred, IpSuffixOriginRandom, SOCKADDR_IN6,
    };

    unsafe {
        let mut size = 16 * 1024u32;
        let mut buf = vec![0u8; size as usize];
        let mut ret = GetAdaptersAddresses(
            AF_INET6.0 as u32,
            GAA_FLAG_INCLUDE_PREFIX,
            None,
            Some(buf.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH),
            &mut size,
        );
        if ret == ERROR_BUFFER_OVERFLOW.0 {
            buf.resize(size as usize, 0);
            ret = GetAdaptersAddresses(
                AF_INET6.0 as u32,
                GAA_FLAG_INCLUDE_PREFIX,
                None,
                Some(buf.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH),
                &mut size,
            );
        }
        if ret != 0 {
            return None;
        }
        let mut adapter = buf.as_ptr() as *const IP_ADAPTER_ADDRESSES_LH;
        while !adapter.is_null() {
            let mut unicast = (*adapter).FirstUnicastAddress;
            while !unicast.is_null() {
                let u = &*unicast;
                if u.DadState == IpDadStatePreferred && u.SuffixOrigin != IpSuffixOriginRandom {
                    let sa = u.Address.lpSockaddr as *const SOCKADDR_IN6;
                    if !sa.is_null() {
                        let bytes = (*sa).sin6_addr.u.Byte;
                        let addr = Ipv6Addr::from(bytes);
                        let seg0 = addr.segments()[0];
                        // Global unicast 2000::/3
                        if (seg0 & 0xe000) == 0x2000 {
                            return Some(addr.to_string());
                        }
                    }
                }
                unicast = u.Next;
            }
            adapter = (*adapter).Next;
        }
        None
    }
}

#[cfg(not(windows))]
pub fn stable_ipv6() -> Option<String> {
    let ifaces = if_addrs::get_if_addrs().ok()?;
    for iface in ifaces {
        if iface.is_loopback() {
            continue;
        }
        if let if_addrs::IfAddr::V6(v6) = iface.addr {
            let seg0 = v6.ip.segments()[0];
            if (seg0 & 0xe000) == 0x2000 {
                return Some(v6.ip.to_string());
            }
        }
    }
    None
}

/// Detecta se o processo está rodando com privilégios elevados (Administrador/Sistema),
/// substituindo a chamada externa ao `whoami.exe /groups`.
#[cfg(windows)]
pub fn is_elevated() -> bool {
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Security::{
        GetSidSubAuthority, GetSidSubAuthorityCount, GetTokenInformation, TOKEN_MANDATORY_LABEL,
        TOKEN_QUERY, TokenIntegrityLevel,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut len = 0u32;
        let _ = GetTokenInformation(token, TokenIntegrityLevel, None, 0, &mut len);
        let mut buf = vec![0u8; len as usize];
        let ok = GetTokenInformation(
            token,
            TokenIntegrityLevel,
            Some(buf.as_mut_ptr() as *mut _),
            len,
            &mut len,
        )
        .is_ok();
        let _ = CloseHandle(token);
        if !ok || buf.len() < std::mem::size_of::<TOKEN_MANDATORY_LABEL>() {
            return false;
        }
        let label = &*(buf.as_ptr() as *const TOKEN_MANDATORY_LABEL);
        let sid = label.Label.Sid;
        let count = *GetSidSubAuthorityCount(sid) as u32;
        if count == 0 {
            return false;
        }
        let rid = *GetSidSubAuthority(sid, count - 1);
        // SECURITY_MANDATORY_HIGH_RID (0x3000 = 12288) or SYSTEM (0x4000 = 16384)
        rid >= 0x3000
    }
}

#[cfg(not(windows))]
pub fn is_elevated() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_ip_checks() {
        assert!(is_private_ip("10.0.0.1"));
        assert!(is_private_ip("192.168.1.1"));
        assert!(is_private_ip("172.16.5.1"));
        assert!(is_private_ip("100.64.0.1")); // CGNAT
        assert!(is_private_ip("fe80::1"));
        assert!(is_private_ip("fc00::1"));
        assert!(!is_private_ip("8.8.8.8"));
        assert!(!is_private_ip("2804:29b8:5086::1"));
    }

    #[test]
    fn url_host_brackets_ipv6() {
        assert_eq!(url_host("1.2.3.4"), "1.2.3.4");
        assert_eq!(url_host("2001:db8::1"), "[2001:db8::1]");
    }
}
