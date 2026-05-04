pub(crate) const VIRTUAL_OUIS: &[[u8; 3]] = &[
    [0x00, 0x05, 0x69],
    [0x00, 0x0C, 0x29],
    [0x00, 0x1C, 0x14],
    [0x00, 0x50, 0x56],
    [0x08, 0x00, 0x27],
    [0x00, 0x15, 0x5D],
    [0x52, 0x54, 0x00],
];

/// Hypervisor vendor strings checked via CPUID leaf 0x40000000. Plain
/// constants — see spec §3.3 on why the historical `obfstr!` wrapping
/// was dropped (open-source: source visibility nullifies obfuscation,
/// and the strings are well-known anyway).
const HYPERVISOR_VENDOR_STRINGS: &[&str] = &[
    "KVMKVMKVM\0\0\0",
    "Microsoft Hv",
    "VMwareVMware",
    "VBoxVBoxVBox",
    "XenVMMXenVMM",
    "prl hyperv  ",
    "TCGTCGTCGTCG",
    "bhyve bhyve ",
];

pub(crate) fn is_virtual_oui(oui: &[u8; 3]) -> bool {
    VIRTUAL_OUIS.iter().any(|v| v == oui)
}

pub(crate) fn is_vm() -> bool {
    hypervisor_cpuid_bit() || hypervisor_vendor_match()
}

fn hypervisor_cpuid_bit() -> bool {
    use raw_cpuid::CpuId;
    CpuId::new()
        .get_feature_info()
        .map(|fi| fi.has_hypervisor())
        .unwrap_or(false)
}

fn hypervisor_vendor_match() -> bool {
    use raw_cpuid::{CpuId, Hypervisor};
    let info = match CpuId::new().get_hypervisor_info() {
        Some(i) => i,
        None => return false,
    };
    let id = info.identify();
    let Hypervisor::Unknown(a, b, c) = id else {
        return true;
    };
    let mut buf = [0u8; 12];
    buf[..4].copy_from_slice(&a.to_le_bytes());
    buf[4..8].copy_from_slice(&b.to_le_bytes());
    buf[8..12].copy_from_slice(&c.to_le_bytes());
    let raw = String::from_utf8_lossy(&buf).into_owned();
    HYPERVISOR_VENDOR_STRINGS
        .iter()
        .any(|v| raw.contains(v.trim_end_matches('\0')))
}
