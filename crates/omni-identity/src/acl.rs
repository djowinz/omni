//! Windows user-only ACL on identity.key.
//!
//! TODO(hardening, follow-up): `verify_user_only` is currently a no-op. The
//! full implementation must enumerate the file's DACL via GetNamedSecurityInfoW
//! and reject the file if any ACE grants READ_DATA to a SID other than the
//! current user or SYSTEM. Tracked in a follow-up ticket opened after sub-spec
//! 006 lands.

use std::path::Path;

use crate::error::IdentityError;

pub(crate) fn set_user_only(path: &Path) -> Result<(), IdentityError> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{GetLastError, PSID};
    use windows_sys::Win32::Security::Authorization::{SetNamedSecurityInfoW, SE_FILE_OBJECT};
    use windows_sys::Win32::Security::{
        AddAccessAllowedAce, CreateWellKnownSid, InitializeAcl, WinCreatorOwnerSid, ACL,
        ACL_REVISION, DACL_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION,
    };
    use windows_sys::Win32::Storage::FileSystem::FILE_ALL_ACCESS;

    if !path.exists() {
        return Err(IdentityError::Permission(format!(
            "cannot set ACL on non-existent file: {}",
            path.display()
        )));
    }

    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    unsafe {
        // CREATOR OWNER well-known SID — resolves to the current user on file-object ACLs.
        let mut sid_buf = [0u8; 68];
        let mut sid_size: u32 = sid_buf.len() as u32;
        let ok = CreateWellKnownSid(
            WinCreatorOwnerSid,
            std::ptr::null_mut(),
            sid_buf.as_mut_ptr() as *mut _,
            &mut sid_size,
        );
        if ok == 0 {
            return Err(IdentityError::Permission(format!(
                "CreateWellKnownSid failed: {}",
                GetLastError()
            )));
        }

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
            sid_buf.as_ptr() as PSID,
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

    Ok(())
}

pub(crate) fn verify_user_only(_path: &Path) -> Result<(), IdentityError> {
    // Best-effort: real verification is a follow-up ticket (see module doc).
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
        assert!(matches!(err, IdentityError::Permission(_)));
    }

    #[test]
    fn verify_user_only_is_noop_for_now() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("identity.key");
        std::fs::write(&p, b"dummy").unwrap();
        verify_user_only(&p).expect("stub should succeed");
    }
}
