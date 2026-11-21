pub mod audio;
pub mod p2p;
pub mod screen;
pub mod session;

/// Detecta se o processo está rodando como Administrador/Sistema no Windows.
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
        rid >= 0x3000
    }
}

#[cfg(not(windows))]
pub fn is_elevated() -> bool {
    false
}
