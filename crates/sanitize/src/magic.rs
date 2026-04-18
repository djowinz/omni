//! Executable-magic deny-list (retro-005 D11 / invariant #19c).

pub(crate) fn reject_executable_magic(data: &[u8]) -> Result<(), &'static [u8]> {
    const SIGS: &[&[u8]] = &[
        &[0x4D, 0x5A],
        &[0x7F, 0x45, 0x4C, 0x46],
        &[0xCA, 0xFE, 0xBA, 0xBE],
        &[0xCF, 0xFA, 0xED, 0xFE],
        &[0xCE, 0xFA, 0xED, 0xFE],
        &[0x50, 0x4B, 0x03, 0x04],
        &[0x1F, 0x8B],
    ];
    for sig in SIGS {
        if data.len() >= sig.len() && &data[..sig.len()] == *sig {
            return Err(sig);
        }
    }
    Ok(())
}
