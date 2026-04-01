/// DLL injection via CreateRemoteThread + LoadLibraryW.
///
/// This module opens the target process, allocates memory for the DLL path,
/// writes the path as UTF-16, and creates a remote thread that calls LoadLibraryW.
///
/// Requires the host process to have sufficient privileges (usually admin or
/// same-user) to open the target process with the necessary access rights.
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;

use tracing::{debug, info};
use windows::core::{s, w};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::Diagnostics::Debug::WriteProcessMemory;
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows::Win32::System::Memory::{
    VirtualAllocEx, VirtualFreeEx, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE,
};
use windows::Win32::System::Threading::{
    CreateRemoteThread, OpenProcess, WaitForSingleObject, PROCESS_ACCESS_RIGHTS,
    PROCESS_CREATE_THREAD, PROCESS_QUERY_INFORMATION, PROCESS_VM_OPERATION, PROCESS_VM_WRITE,
};

use crate::error::HostError;
use crate::win32::{self, OwnedHandle};

/// RAII guard for memory allocated in a remote process via `VirtualAllocEx`.
struct RemoteAlloc {
    process: HANDLE,
    ptr: *mut std::ffi::c_void,
}

impl Drop for RemoteAlloc {
    fn drop(&mut self) {
        // SAFETY: `self.process` is a valid handle for the lifetime of the
        // injection operation. `self.ptr` was returned by `VirtualAllocEx`.
        unsafe {
            let _ = VirtualFreeEx(self.process, self.ptr, 0, MEM_RELEASE);
        }
    }
}

/// Inject a DLL into a target process.
///
/// # Arguments
/// * `pid` - Process ID of the target game
/// * `dll_path` - Absolute path to the DLL file on disk
///
/// # Errors
/// Returns an error if any Win32 API call fails (insufficient privileges,
/// invalid PID, etc.)
pub fn inject_dll(pid: u32, dll_path: &str) -> Result<(), HostError> {
    let dll_filename = std::path::Path::new(dll_path)
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or("omni_overlay.dll");

    if win32::has_module(pid, dll_filename).unwrap_or(false) {
        info!(
            pid,
            dll_filename, "DLL already loaded in target — skipping injection"
        );
        return Ok(());
    }

    let wide_path: Vec<u16> = OsStr::new(dll_path)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let path_byte_size = wide_path.len() * std::mem::size_of::<u16>();

    debug!(pid, dll_path, path_byte_size, "Opening target process");

    const INJECT_RIGHTS: PROCESS_ACCESS_RIGHTS = PROCESS_ACCESS_RIGHTS(
        PROCESS_CREATE_THREAD.0
            | PROCESS_VM_OPERATION.0
            | PROCESS_VM_WRITE.0
            | PROCESS_QUERY_INFORMATION.0,
    );
    // SAFETY: OpenProcess with minimum required rights on a valid PID.
    let process = OwnedHandle::new(unsafe { OpenProcess(INJECT_RIGHTS, false, pid)? });

    // SAFETY: Allocating read/write memory in the target process.
    let remote_mem = unsafe {
        VirtualAllocEx(
            process.raw(),
            None,
            path_byte_size,
            MEM_COMMIT | MEM_RESERVE,
            PAGE_READWRITE,
        )
    };
    if remote_mem.is_null() {
        return Err("VirtualAllocEx failed — could not allocate memory in target process".into());
    }
    let _alloc = RemoteAlloc {
        process: process.raw(),
        ptr: remote_mem,
    };

    debug!(?remote_mem, "Allocated memory in target process");

    // SAFETY: Writing the UTF-16 DLL path into the allocated region.
    unsafe {
        WriteProcessMemory(
            process.raw(),
            remote_mem,
            wide_path.as_ptr() as *const _,
            path_byte_size,
            None,
        )
    }
    .map_err(|e| HostError::Message(format!("WriteProcessMemory failed: {e}")))?;

    debug!("Wrote DLL path to target process memory");

    // SAFETY: kernel32.dll has a consistent base address within a boot session.
    let kernel32 = unsafe { GetModuleHandleW(w!("kernel32.dll"))? };
    let load_library_addr = unsafe { GetProcAddress(kernel32, s!("LoadLibraryW")) }.ok_or(
        HostError::Message("GetProcAddress: could not find LoadLibraryW".into()),
    )?;

    debug!(?load_library_addr, "Found LoadLibraryW address");

    // SAFETY: load_library_addr points to LoadLibraryW which has the same
    // calling convention as LPTHREAD_START_ROUTINE (one pointer param, returns pointer).
    let thread = OwnedHandle::new(unsafe {
        CreateRemoteThread(
            process.raw(),
            None,
            0,
            Some(std::mem::transmute::<
                unsafe extern "system" fn() -> isize,
                unsafe extern "system" fn(*mut std::ffi::c_void) -> u32,
            >(load_library_addr)),
            Some(remote_mem),
            0,
            None,
        )?
    });

    info!("Created remote thread — waiting for DLL to load");

    // SAFETY: Waiting for the remote thread to complete.
    unsafe {
        WaitForSingleObject(thread.raw(), 10_000);
    }

    info!("DLL injection complete");
    Ok(())
}

/// Eject the overlay DLL from a target process by calling its exported
/// `omni_shutdown` function via CreateRemoteThread.
///
/// `omni_shutdown` disables all minhook trampolines, sleeps to let in-flight
/// hook calls drain, then calls `FreeLibraryAndExitThread` to atomically
/// unload the DLL and exit the thread — no dangling vtable pointers.
///
/// # Arguments
/// * `pid` - Process ID of the target
/// * `dll_name` - Filename of the DLL to eject (e.g. "omni_overlay.dll")
pub fn eject_dll(pid: u32, dll_name: &str) -> Result<(), HostError> {
    let shutdown_addr = find_remote_export(pid, dll_name, "omni_shutdown")?.ok_or_else(|| {
        HostError::Message(format!(
            "'omni_shutdown' export not found in {} (pid {})",
            dll_name, pid
        ))
    })?;

    debug!(?shutdown_addr, "Found omni_shutdown address");

    const EJECT_RIGHTS: PROCESS_ACCESS_RIGHTS =
        PROCESS_ACCESS_RIGHTS(PROCESS_CREATE_THREAD.0 | PROCESS_QUERY_INFORMATION.0);
    // SAFETY: Opening the target process with minimum required rights.
    let process = OwnedHandle::new(unsafe { OpenProcess(EJECT_RIGHTS, false, pid)? });

    // SAFETY: shutdown_addr is the resolved address of omni_shutdown.
    let thread = OwnedHandle::new(unsafe {
        CreateRemoteThread(
            process.raw(),
            None,
            0,
            Some(std::mem::transmute::<
                *const std::ffi::c_void,
                unsafe extern "system" fn(*mut std::ffi::c_void) -> u32,
            >(shutdown_addr)),
            None,
            0,
            None,
        )?
    });

    info!("Created remote thread calling omni_shutdown — waiting for clean unload");

    // SAFETY: omni_shutdown sleeps 200ms then calls FreeLibraryAndExitThread.
    unsafe {
        WaitForSingleObject(thread.raw(), 10_000);
    }

    info!("DLL ejection complete");
    Ok(())
}

/// Find the address of an exported function in a module loaded in a remote process.
///
/// Reads the PE export directory from the DLL file on disk to find the export's RVA,
/// then adds it to the module's base address in the remote process.
fn find_remote_export(
    pid: u32,
    dll_name: &str,
    export_name: &str,
) -> Result<Option<*const std::ffi::c_void>, HostError> {
    let (remote_base, dll_path) = match win32::find_remote_module(pid, dll_name)? {
        Some((base, path)) => (base as usize, path),
        None => return Ok(None),
    };

    let rva = find_export_rva_from_file(&dll_path, export_name)?.ok_or_else(|| {
        HostError::Message(format!(
            "Export '{}' not found in '{}'",
            export_name, dll_path
        ))
    })?;

    let remote_addr = (remote_base + rva as usize) as *const std::ffi::c_void;
    Ok(Some(remote_addr))
}

/// Parse a PE file on disk and find the RVA of a named export.
fn find_export_rva_from_file(dll_path: &str, export_name: &str) -> Result<Option<u32>, HostError> {
    let data = std::fs::read(dll_path)?;

    if data.len() < 0x40 {
        return Err("File too small for DOS header".into());
    }
    let e_lfanew = read_u32(&data, 0x3C)? as usize;

    let coff_start = e_lfanew + 4;
    if data.len() < coff_start + 20 {
        return Err("File too small for COFF header".into());
    }

    let optional_hdr_start = coff_start + 20;
    let magic = read_u16(&data, optional_hdr_start)?;

    let export_dir_offset = match magic {
        0x20B => optional_hdr_start + 112,
        0x10B => optional_hdr_start + 96,
        _ => return Err(format!("Unknown PE optional header magic: {magic:#x}").into()),
    };

    let export_rva = read_u32(&data, export_dir_offset)? as usize;
    let export_size = read_u32(&data, export_dir_offset + 4)? as usize;

    if export_rva == 0 || export_size == 0 {
        return Ok(None);
    }

    let num_sections = read_u16(&data, coff_start + 2)? as usize;
    let optional_hdr_size = read_u16(&data, coff_start + 16)? as usize;
    let sections_start = optional_hdr_start + optional_hdr_size;

    let rva_to_offset = |rva: usize| -> Option<usize> {
        for i in 0..num_sections {
            let s = sections_start + i * 40;
            let vaddr = u32::from_le_bytes(data.get(s + 12..s + 16)?.try_into().ok()?) as usize;
            let vsize = u32::from_le_bytes(data.get(s + 8..s + 12)?.try_into().ok()?) as usize;
            let raw_ptr = u32::from_le_bytes(data.get(s + 20..s + 24)?.try_into().ok()?) as usize;
            if rva >= vaddr && rva < vaddr + vsize {
                return Some(rva - vaddr + raw_ptr);
            }
        }
        None
    };

    let export_offset = rva_to_offset(export_rva)
        .ok_or_else(|| HostError::Message("Could not map export directory RVA".into()))?;

    let num_names = read_u32(&data, export_offset + 24)? as usize;
    let addr_of_functions_rva = read_u32(&data, export_offset + 28)? as usize;
    let addr_of_names_rva = read_u32(&data, export_offset + 32)? as usize;
    let addr_of_ordinals_rva = read_u32(&data, export_offset + 36)? as usize;

    let names_offset = rva_to_offset(addr_of_names_rva)
        .ok_or_else(|| HostError::Message("Could not map names RVA".into()))?;
    let ordinals_offset = rva_to_offset(addr_of_ordinals_rva)
        .ok_or_else(|| HostError::Message("Could not map ordinals RVA".into()))?;
    let functions_offset = rva_to_offset(addr_of_functions_rva)
        .ok_or_else(|| HostError::Message("Could not map functions RVA".into()))?;

    for i in 0..num_names {
        let name_rva = read_u32(&data, names_offset + i * 4)? as usize;
        let name_offset = rva_to_offset(name_rva)
            .ok_or_else(|| HostError::Message("Could not map export name RVA".into()))?;

        let name_end = data
            .get(name_offset..)
            .and_then(|s| s.iter().position(|&b| b == 0))
            .map(|pos| name_offset + pos)
            .unwrap_or(name_offset);
        let name = std::str::from_utf8(data.get(name_offset..name_end).unwrap_or(&[]))
            .map_err(|e| HostError::Message(format!("Invalid UTF-8 in export name: {e}")))?;

        if name == export_name {
            let ordinal = read_u16(&data, ordinals_offset + i * 2)? as usize;
            let func_rva = read_u32(&data, functions_offset + ordinal * 4)?;
            return Ok(Some(func_rva));
        }
    }

    Ok(None)
}

/// Read a little-endian u32 from `data` at `offset` with bounds checking.
fn read_u32(data: &[u8], offset: usize) -> Result<u32, HostError> {
    data.get(offset..offset + 4)
        .and_then(|s| s.try_into().ok())
        .map(u32::from_le_bytes)
        .ok_or_else(|| HostError::Message(format!("PE read out of bounds at offset {offset:#x}")))
}

/// Read a little-endian u16 from `data` at `offset` with bounds checking.
fn read_u16(data: &[u8], offset: usize) -> Result<u16, HostError> {
    data.get(offset..offset + 2)
        .and_then(|s| s.try_into().ok())
        .map(u16::from_le_bytes)
        .ok_or_else(|| HostError::Message(format!("PE read out of bounds at offset {offset:#x}")))
}
