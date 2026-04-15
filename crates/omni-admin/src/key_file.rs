//! Operator key-file permission hygiene.
//!
//! Rejects overbroad permissions on the operator key file before the CLI
//! reads or writes it. This is the same hygiene pattern used by:
//!
//! - **OpenSSH** — `StrictModes` refuses to load `~/.ssh/id_ed25519` unless
//!   the file mode is `0600` (see `ssh-keygen(1)` / `sshd_config(5)`).
//! - **GnuPG** — `~/.gnupg` enforces mode `0700` and warns on looser perms
//!   (`gpg --list-keys` emits "unsafe permissions" otherwise).
//! - **kubectl** — emits a "config file permissions are too open" warning
//!   when the kubeconfig is group- or world-readable.
//!
//! On Unix we check the file mode directly. On Windows we read the file's
//! DACL via `GetNamedSecurityInfoW`, walk the ACEs, and reject any
//! `ACCESS_ALLOWED_ACE` that grants read/write to a well-known overbroad
//! principal (`Everyone`, built-in `Users`, `Authenticated Users`).
//!
//! The public entry point is [`check_permissions`]. Task 12 only lands the
//! check — T13+ wire it into commands that actually load the key file.

use std::path::Path;

/// Verify the key-file at `path` is not world/group accessible.
///
/// Returns `Ok(())` if the file's permissions are restricted to the current
/// user (Unix: `mode & 0o077 == 0`; Windows: no DACL ACE grants read/write
/// to `Everyone`, `Users`, or `Authenticated Users`).
///
/// Errors include the current mode / offending principal and a remediation
/// hint (`chmod 600 <path>` on Unix, `icacls … /inheritance:r …` on Windows).
pub fn check_permissions(path: &Path) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        check_unix(path)
    }

    #[cfg(windows)]
    {
        check_windows(path)
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = path;
        Ok(())
    }
}

#[cfg(unix)]
fn check_unix(path: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let meta = std::fs::metadata(path)
        .map_err(|e| anyhow::anyhow!("cannot stat key file {}: {e}", path.display()))?;
    let mode = meta.permissions().mode() & 0o777;
    if mode & 0o077 != 0 {
        anyhow::bail!(
            "key file {} has overbroad mode {:04o} (group/other bits set); \
             remediation: `chmod 600 {}`",
            path.display(),
            mode,
            path.display()
        );
    }
    Ok(())
}

#[cfg(windows)]
fn check_windows(path: &Path) -> anyhow::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PWSTR;
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::Authorization::{GetNamedSecurityInfoW, SE_FILE_OBJECT};
    use windows::Win32::Security::{
        AclSizeInformation, GetAce, GetAclInformation, LookupAccountSidW, ACCESS_ALLOWED_ACE,
        ACE_HEADER, ACL, ACL_SIZE_INFORMATION, DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
        PSID, SID_NAME_USE,
    };
    use windows::Win32::Storage::FileSystem::{FILE_READ_DATA, FILE_WRITE_DATA};
    use windows::Win32::System::SystemServices::ACCESS_ALLOWED_ACE_TYPE;

    // Rights that constitute "read or write" for our purposes.
    const GENERIC_READ_MASK: u32 = 0x8000_0000;
    const GENERIC_WRITE_MASK: u32 = 0x4000_0000;
    const GENERIC_ALL_MASK: u32 = 0x1000_0000;

    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let wide_pcwstr = windows::core::PCWSTR(wide.as_ptr());

    let mut pdacl: *mut ACL = std::ptr::null_mut();
    let mut psd = PSECURITY_DESCRIPTOR::default();

    unsafe {
        let status = GetNamedSecurityInfoW(
            wide_pcwstr,
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            None,
            None,
            Some(&mut pdacl),
            None,
            &mut psd,
        );
        if status.is_err() {
            return Err(anyhow::anyhow!(
                "GetNamedSecurityInfoW failed for {}: {:?}",
                path.display(),
                status
            ));
        }

        // Guard against a null DACL — that means "everyone has full access".
        if pdacl.is_null() {
            if !psd.0.is_null() {
                let _ = LocalFree(HLOCAL(psd.0));
            }
            anyhow::bail!(
                "key file {} has a NULL DACL (world-accessible); remediation: \
                 `icacls {} /inheritance:r /grant:r %USERNAME%:R`",
                path.display(),
                path.display()
            );
        }

        let mut info = ACL_SIZE_INFORMATION::default();
        let info_size = std::mem::size_of::<ACL_SIZE_INFORMATION>() as u32;
        if GetAclInformation(
            pdacl,
            &mut info as *mut _ as *mut _,
            info_size,
            AclSizeInformation,
        )
        .is_err()
        {
            let _ = LocalFree(HLOCAL(psd.0));
            anyhow::bail!(
                "GetAclInformation failed for {}: {}",
                path.display(),
                std::io::Error::last_os_error()
            );
        }

        let bad_mask = FILE_READ_DATA.0
            | FILE_WRITE_DATA.0
            | GENERIC_READ_MASK
            | GENERIC_WRITE_MASK
            | GENERIC_ALL_MASK;

        for i in 0..info.AceCount {
            let mut pace: *mut core::ffi::c_void = std::ptr::null_mut();
            if GetAce(pdacl, i, &mut pace).is_err() {
                let _ = LocalFree(HLOCAL(psd.0));
                anyhow::bail!(
                    "GetAce({i}) failed for {}: {}",
                    path.display(),
                    std::io::Error::last_os_error()
                );
            }

            let header = pace as *const ACE_HEADER;
            if (*header).AceType != ACCESS_ALLOWED_ACE_TYPE as u8 {
                continue;
            }

            let ace = pace as *const ACCESS_ALLOWED_ACE;
            let mask = (*ace).Mask;
            if mask & bad_mask == 0 {
                continue;
            }

            let sid_ptr = PSID(&(*ace).SidStart as *const u32 as *mut core::ffi::c_void);

            // Resolve the SID to account name + domain.
            let mut name_buf = [0u16; 256];
            let mut domain_buf = [0u16; 256];
            let mut name_len = name_buf.len() as u32;
            let mut domain_len = domain_buf.len() as u32;
            let mut sid_type = SID_NAME_USE::default();

            let resolved = LookupAccountSidW(
                windows::core::PCWSTR::null(),
                sid_ptr,
                PWSTR(name_buf.as_mut_ptr()),
                &mut name_len,
                PWSTR(domain_buf.as_mut_ptr()),
                &mut domain_len,
                &mut sid_type,
            );

            // If we cannot resolve the SID, conservatively continue — an
            // unknown SID isn't one of the well-known overbroad principals.
            if resolved.is_err() {
                continue;
            }

            let name = String::from_utf16_lossy(&name_buf[..name_len as usize]);
            let domain = String::from_utf16_lossy(&domain_buf[..domain_len as usize]);

            let name_lc = name.to_ascii_lowercase();
            let is_overbroad = matches!(
                name_lc.as_str(),
                "everyone" | "users" | "authenticated users"
            );

            if is_overbroad {
                let _ = LocalFree(HLOCAL(psd.0));
                let principal = if domain.is_empty() {
                    name
                } else {
                    format!("{domain}\\{name}")
                };
                anyhow::bail!(
                    "key file {} DACL grants read/write to overbroad principal `{}`; \
                     remediation: `icacls {} /inheritance:r /grant:r %USERNAME%:R`",
                    path.display(),
                    principal,
                    path.display()
                );
            }
        }

        let _ = LocalFree(HLOCAL(psd.0));
    }

    Ok(())
}
