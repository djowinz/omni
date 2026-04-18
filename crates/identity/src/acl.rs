//! Windows user-only ACL on identity.key.

use std::path::Path;

use crate::error::IdentityError;

/// Returns a heap-allocated copy of the current process token's user SID bytes.
/// The returned `Vec<u8>` is correctly aligned and can be cast to `PSID`.
#[cfg(windows)]
fn current_user_sid_bytes() -> Result<Vec<u8>, IdentityError> {
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, HANDLE};
    use windows_sys::Win32::Security::GetLengthSid;
    use windows_sys::Win32::Security::{GetTokenInformation, TokenUser, TOKEN_QUERY, TOKEN_USER};
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    unsafe {
        let mut token: HANDLE = 0;
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(IdentityError::Permission(format!(
                "OpenProcessToken failed: {}",
                GetLastError()
            )));
        }

        // First call: get required buffer size.
        let mut needed: u32 = 0;
        GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut needed);
        if needed == 0 {
            CloseHandle(token);
            return Err(IdentityError::Permission(format!(
                "GetTokenInformation size failed: {}",
                GetLastError()
            )));
        }

        // Second call: fill the buffer.
        let mut buf: Vec<u8> = vec![0u8; needed as usize];
        if GetTokenInformation(
            token,
            TokenUser,
            buf.as_mut_ptr() as *mut _,
            needed,
            &mut needed,
        ) == 0
        {
            CloseHandle(token);
            return Err(IdentityError::Permission(format!(
                "GetTokenInformation failed: {}",
                GetLastError()
            )));
        }

        // TOKEN_USER begins with SID_AND_ATTRIBUTES; first field is Sid (PSID).
        let tu = &*(buf.as_ptr() as *const TOKEN_USER);
        let sid_ptr = tu.User.Sid;
        let sid_len = GetLengthSid(sid_ptr) as usize;

        // Copy just the SID bytes into a fresh, well-aligned Vec.
        let mut sid_bytes = vec![0u8; sid_len];
        std::ptr::copy_nonoverlapping(sid_ptr as *const u8, sid_bytes.as_mut_ptr(), sid_len);

        CloseHandle(token);
        Ok(sid_bytes)
    }
}

pub(crate) fn set_user_only(path: &Path) -> Result<(), IdentityError> {
    if !path.exists() {
        return Err(IdentityError::Io(format!(
            "cannot set ACL on non-existent file: {}",
            path.display()
        )));
    }

    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Foundation::{GetLastError, PSID};
        use windows_sys::Win32::Security::Authorization::{SetNamedSecurityInfoW, SE_FILE_OBJECT};
        use windows_sys::Win32::Security::{
            AddAccessAllowedAce, InitializeAcl, ACL, ACL_REVISION, DACL_SECURITY_INFORMATION,
            PROTECTED_DACL_SECURITY_INFORMATION,
        };
        use windows_sys::Win32::Storage::FileSystem::FILE_ALL_ACCESS;

        let wide: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        let sid_bytes = current_user_sid_bytes()?;
        let sid_ptr = sid_bytes.as_ptr() as PSID;

        unsafe {
            let mut acl_buf = [0u8; 1024];
            if InitializeAcl(
                acl_buf.as_mut_ptr() as *mut ACL,
                acl_buf.len() as u32,
                ACL_REVISION,
            ) == 0
            {
                return Err(IdentityError::Permission(format!(
                    "InitializeAcl failed: {}",
                    GetLastError()
                )));
            }

            if AddAccessAllowedAce(
                acl_buf.as_mut_ptr() as *mut ACL,
                ACL_REVISION,
                FILE_ALL_ACCESS,
                sid_ptr,
            ) == 0
            {
                return Err(IdentityError::Permission(format!(
                    "AddAccessAllowedAce failed: {}",
                    GetLastError()
                )));
            }

            let status = SetNamedSecurityInfoW(
                wide.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                acl_buf.as_ptr() as *const ACL,
                std::ptr::null_mut(),
            );
            if status != 0 {
                return Err(IdentityError::Permission(format!(
                    "SetNamedSecurityInfoW failed: {status}"
                )));
            }
        }
    }

    Ok(())
}

pub(crate) fn verify_user_only(path: &Path) -> Result<(), IdentityError> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Foundation::{GetLastError, LocalFree, PSID};
        use windows_sys::Win32::Security::Authorization::{GetNamedSecurityInfoW, SE_FILE_OBJECT};
        use windows_sys::Win32::Security::{
            AclSizeInformation, CreateWellKnownSid, EqualSid, GetAce, GetAclInformation,
            WinLocalSystemSid, ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, ACL_SIZE_INFORMATION,
            DACL_SECURITY_INFORMATION,
        };
        use windows_sys::Win32::System::SystemServices::ACCESS_ALLOWED_ACE_TYPE;

        let wide: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        let cur_sid_bytes = current_user_sid_bytes()?;
        let cur_sid_ptr = cur_sid_bytes.as_ptr() as PSID;

        unsafe {
            // Build LocalSystem well-known SID.
            let mut system_sid_buf = [0u8; 68];
            let mut system_sid_size = system_sid_buf.len() as u32;
            if CreateWellKnownSid(
                WinLocalSystemSid,
                std::ptr::null_mut(),
                system_sid_buf.as_mut_ptr() as *mut _,
                &mut system_sid_size,
            ) == 0
            {
                return Err(IdentityError::Permission(format!(
                    "CreateWellKnownSid(LocalSystem) failed: {}",
                    GetLastError()
                )));
            }
            let system_sid_ptr = system_sid_buf.as_ptr() as PSID;

            // Read the file's DACL.
            let mut pdacl: *mut ACL = std::ptr::null_mut();
            let mut psd: *mut std::ffi::c_void = std::ptr::null_mut();
            let status = GetNamedSecurityInfoW(
                wide.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut pdacl,
                std::ptr::null_mut(),
                &mut psd,
            );
            if status != 0 {
                return Err(IdentityError::Permission(format!(
                    "GetNamedSecurityInfoW failed: {status}"
                )));
            }

            // Walk ACEs.
            let mut info = ACL_SIZE_INFORMATION {
                AceCount: 0,
                AclBytesInUse: 0,
                AclBytesFree: 0,
            };
            let info_size = std::mem::size_of::<ACL_SIZE_INFORMATION>() as u32;
            if GetAclInformation(
                pdacl,
                &mut info as *mut _ as *mut _,
                info_size,
                AclSizeInformation,
            ) == 0
            {
                LocalFree(psd as *mut _);
                return Err(IdentityError::Permission(format!(
                    "GetAclInformation failed: {}",
                    GetLastError()
                )));
            }

            for i in 0..info.AceCount {
                let mut pace: *mut std::ffi::c_void = std::ptr::null_mut();
                if GetAce(pdacl, i, &mut pace) == 0 {
                    LocalFree(psd as *mut _);
                    return Err(IdentityError::Permission(format!(
                        "GetAce({i}) failed: {}",
                        GetLastError()
                    )));
                }

                let header = pace as *const ACE_HEADER;
                if (*header).AceType == ACCESS_ALLOWED_ACE_TYPE as u8 {
                    let ace = pace as *const ACCESS_ALLOWED_ACE;
                    let sid_ptr = &(*ace).SidStart as *const u32 as PSID;
                    if EqualSid(sid_ptr, cur_sid_ptr) == 0 && EqualSid(sid_ptr, system_sid_ptr) == 0
                    {
                        LocalFree(psd as *mut _);
                        return Err(IdentityError::Permission(
                            "file DACL grants access to foreign SID".to_string(),
                        ));
                    }
                }
            }

            LocalFree(psd as *mut _);
        }
    }

    #[cfg(not(windows))]
    {
        let _ = path;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn set_user_only_succeeds_on_valid_file() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("identity.key");
        std::fs::write(&p, b"dummy").unwrap();
        set_user_only(&p).expect("should succeed on a normal file");
    }

    #[test]
    fn set_user_only_errors_on_missing_file() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("missing.key");
        let err = set_user_only(&p).unwrap_err();
        assert!(matches!(err, IdentityError::Io(_)));
    }

    #[test]
    fn verify_user_only_accepts_file_we_set() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("identity.key");
        std::fs::write(&p, b"dummy").unwrap();
        set_user_only(&p).expect("set_user_only should succeed");
        verify_user_only(&p).expect("verify_user_only should accept a file we just locked down");
    }
}
