//! PE .text self-integrity.
//!
//! Per retro D-004-C: under `strict-integrity` feature use `env!`
//! (compile-time required); otherwise `option_env!` (silent skip).
//!
//! Per retro D-004-J: `GuardError::IntegrityFailed` stays opaque.
//! Diagnostic (expected/actual/section_size) goes through tracing::debug!
//! only when strict-integrity is OFF. Release builds emit nothing.

use crate::types::GuardError;
use sha2::{Digest, Sha256};

#[cfg(feature = "strict-integrity")]
const EXPECTED_TEXT_SHA256_HEX: Option<&str> = Some(env!("OMNI_GUARD_TEXT_SHA256"));

#[cfg(not(feature = "strict-integrity"))]
const EXPECTED_TEXT_SHA256_HEX: Option<&str> = option_env!("OMNI_GUARD_TEXT_SHA256");

pub(crate) fn verify_text_section() -> Result<(), GuardError> {
    let expected_hex = match EXPECTED_TEXT_SHA256_HEX {
        Some(h) if !h.is_empty() => h,
        _ => return Ok(()),
    };
    let actual = hash_own_text_section().map_err(|e| GuardError::Other(e.to_string()))?;
    let mut expected = [0u8; 32];
    if hex_decode_32(expected_hex, &mut expected).is_err() {
        return Err(GuardError::Other(
            "invalid expected integrity hash format".to_string(),
        ));
    }
    if actual == expected {
        Ok(())
    } else {
        #[cfg(not(feature = "strict-integrity"))]
        {
            let actual_hex: String = actual.iter().map(|b| format!("{b:02x}")).collect();
            tracing::debug!(
                expected = %expected_hex,
                actual = %actual_hex,
                "omni-guard integrity mismatch (strict-integrity OFF so diagnostic is logged)"
            );
        }
        Err(GuardError::IntegrityFailed)
    }
}

fn hex_decode_32(hex: &str, out: &mut [u8; 32]) -> Result<(), ()> {
    if hex.len() != 64 {
        return Err(());
    }
    for (i, chunk) in hex.as_bytes().chunks(2).enumerate() {
        let s = std::str::from_utf8(chunk).map_err(|_| ())?;
        out[i] = u8::from_str_radix(s, 16).map_err(|_| ())?;
    }
    Ok(())
}

fn hash_own_text_section() -> Result<[u8; 32], &'static str> {
    // NOTE: hashing in-memory .text assumes no base-relocation fixups have
    // touched code bytes. For x64 /DYNAMICBASE images this holds because
    // code is RIP-relative. ARM64 or 32-bit builds would need to hash the
    // on-disk image instead.
    use goblin::pe::PE;
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;

    let module = unsafe { GetModuleHandleW(windows::core::PCWSTR::null()) }
        .map_err(|_| "GetModuleHandle failed")?;
    let base = module.0 as *const u8;
    let size = unsafe { image_size_from_pe_header(base) };
    let mem = unsafe { std::slice::from_raw_parts(base, size) };

    // Loaded in-memory image: file offsets don't match, symbol/import
    // tables can appear "out of bounds" to goblin. Use lenient options
    // (no RVA resolution) to extract section headers only.
    let opts = goblin::pe::options::ParseOptions {
        resolve_rva: false,
        parse_attribute_certificates: false,
    };
    let pe = PE::parse_with_opts(mem, &opts).map_err(|_| "pe parse failed")?;
    let text = pe
        .sections
        .iter()
        .find(|s| s.name().unwrap_or_default() == ".text")
        .ok_or(".text not found")?;
    let start = text.virtual_address as usize;
    let end = start + text.virtual_size as usize;
    if end > mem.len() {
        return Err(".text out of bounds");
    }
    let mut h = Sha256::new();
    h.update(&mem[start..end]);
    Ok(h.finalize().into())
}

unsafe fn image_size_from_pe_header(base: *const u8) -> usize {
    // DOS_HEADER.e_lfanew offset = 0x3C.
    // NT headers: PE\0\0 (4) + FileHeader (20) + OptionalHeader.SizeOfImage @+56 = 80.
    let e_lfanew = *(base.add(0x3C) as *const u32) as usize;
    *(base.add(e_lfanew + 80) as *const u32) as usize
}
