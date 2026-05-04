use sha2::{Digest, Sha256};

use crate::{antidebug, vm};

#[inline(never)]
pub(crate) fn telemetry_session_seed() -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(b"omni-guard-v1");
    h.update(primary_physical_mac());
    h.update(machine_guid_hash());
    h.update(cpu_brand_hash());
    let mut out: [u8; 32] = h.finalize().into();
    if antidebug::any_check_positive() {
        out[0] ^= 0x5A; // silent poison
    }
    out
}

fn primary_physical_mac() -> [u8; 6] {
    use windows::Win32::NetworkManagement::IpHelper::{
        GetAdaptersAddresses, GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_SERVER,
        GAA_FLAG_SKIP_MULTICAST, IP_ADAPTER_ADDRESSES_LH,
    };
    use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
    use windows::Win32::Networking::WinSock::AF_UNSPEC;

    // IANA interface types. `windows-rs` 0.52 doesn't re-export these
    // consistently, so inline the values.
    const IF_TYPE_ETHERNET_CSMACD: u32 = 6;
    const IF_TYPE_IEEE80211: u32 = 71;

    let flags = GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER;

    let mut size: u32 = 0;
    unsafe {
        GetAdaptersAddresses(AF_UNSPEC.0 as u32, flags, None, None, &mut size);
    }
    if size == 0 {
        return [0; 6];
    }
    let mut buf = vec![0u8; size as usize];
    let head = buf.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH;
    let rc = unsafe {
        GetAdaptersAddresses(AF_UNSPEC.0 as u32, flags, None, Some(head), &mut size)
    };
    if rc != 0 {
        return [0; 6];
    }

    let mut cur = head;
    while !cur.is_null() {
        let a = unsafe { &*cur };
        let is_up = a.OperStatus == IfOperStatusUp;
        let len = a.PhysicalAddressLength as usize;
        let is_physical_type = a.IfType == IF_TYPE_ETHERNET_CSMACD || a.IfType == IF_TYPE_IEEE80211;
        if is_up && is_physical_type && len >= 6 {
            let mut mac = [0u8; 6];
            mac.copy_from_slice(&a.PhysicalAddress[..6]);
            if !vm::is_virtual_oui(&[mac[0], mac[1], mac[2]]) {
                return mac;
            }
        }
        cur = a.Next;
    }
    [0; 6]
}

fn machine_guid_hash() -> [u8; 32] {
    use windows::core::PCWSTR;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_LOCAL_MACHINE, KEY_READ,
        REG_VALUE_TYPE,
    };

    fn to_w(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }
    let subkey = to_w(r"SOFTWARE\Microsoft\Cryptography");
    let value = to_w("MachineGuid");

    let mut hkey: HKEY = HKEY::default();
    if unsafe {
        RegOpenKeyExW(HKEY_LOCAL_MACHINE, PCWSTR(subkey.as_ptr()), 0, KEY_READ, &mut hkey)
    }
    .is_err()
    {
        return [0; 32];
    }
    let mut kind = REG_VALUE_TYPE::default();
    let mut size: u32 = 0;
    let _ = unsafe {
        RegQueryValueExW(
            hkey,
            PCWSTR(value.as_ptr()),
            None,
            Some(&mut kind),
            None,
            Some(&mut size),
        )
    };
    if size == 0 {
        unsafe {
            let _ = RegCloseKey(hkey);
        }
        return [0; 32];
    }
    let mut buf = vec![0u8; size as usize];
    let _ = unsafe {
        RegQueryValueExW(
            hkey,
            PCWSTR(value.as_ptr()),
            None,
            Some(&mut kind),
            Some(buf.as_mut_ptr()),
            Some(&mut size),
        )
    };
    unsafe {
        let _ = RegCloseKey(hkey);
    }

    let mut h = Sha256::new();
    h.update(&buf);
    h.finalize().into()
}

fn cpu_brand_hash() -> [u8; 16] {
    use raw_cpuid::CpuId;
    let brand = CpuId::new()
        .get_processor_brand_string()
        .map(|b| b.as_str().to_string())
        .unwrap_or_default();
    let h: [u8; 32] = Sha256::digest(brand.as_bytes()).into();
    let mut out = [0u8; 16];
    out.copy_from_slice(&h[..16]);
    out
}
