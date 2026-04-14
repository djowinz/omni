//! integrity-hash: print SHA-256 of a PE file's `.text` section.
//!
//! Usage: integrity-hash <path-to-pe-binary>
//!
//! Used by omni-host's release CI to bake the Stage-1 binary's .text digest
//! into the Stage-2 build via the `OMNI_GUARD_TEXT_SHA256` environment
//! variable (retro/2026-04-13-theme-sharing-004-design-retro.md D-004-F).

use std::{env, fs, process::ExitCode};

use goblin::pe::PE;
use sha2::{Digest, Sha256};

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let path = match args.next() {
        Some(p) => p,
        None => {
            eprintln!("usage: integrity-hash <path-to-pe-binary>");
            return ExitCode::from(2);
        }
    };
    match run(&path) {
        Ok(hex) => {
            println!("{hex}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("integrity-hash: {e}");
            ExitCode::from(1)
        }
    }
}

fn run(path: &str) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|e| format!("read {path}: {e}"))?;
    let pe = PE::parse(&bytes).map_err(|e| format!("parse pe {path}: {e}"))?;
    let text = pe
        .sections
        .iter()
        .find(|s| s.name().unwrap_or_default() == ".text")
        .ok_or_else(|| format!(".text section not found in {path}"))?;

    // Use pointer_to_raw_data + size_of_raw_data — the on-disk bytes that the
    // loader maps into .text. Memory-layout (virtual_address / virtual_size)
    // may include uninitialized padding; disk bytes are stable across runs.
    let start = text.pointer_to_raw_data as usize;
    let end = start
        .checked_add(text.size_of_raw_data as usize)
        .ok_or_else(|| "pe .text range overflow".to_string())?;
    if end > bytes.len() {
        return Err(format!(
            ".text out of bounds: end={end} file_size={}",
            bytes.len()
        ));
    }

    let mut h = Sha256::new();
    h.update(&bytes[start..end]);
    let digest = h.finalize();

    let mut hex = String::with_capacity(64);
    for b in digest {
        use std::fmt::Write;
        let _ = write!(hex, "{b:02x}");
    }
    Ok(hex)
}
