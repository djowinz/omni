/// DLL injection via CreateRemoteThread + LoadLibraryW.
///
/// This module opens the target process, allocates memory for the DLL path,
/// writes the path as UTF-16, and creates a remote thread that calls LoadLibraryW.
///
/// Requires the host process to have sufficient privileges (usually admin or
/// same-user) to open the target process with the necessary access rights.

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::ptr;

use std::mem::size_of;

use tracing::{debug, info};
use windows::Win32::Foundation::{CloseHandle, HANDLE, ERROR_NO_MORE_FILES};
use windows::Win32::System::Diagnostics::Debug::WriteProcessMemory;
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Module32FirstW, Module32NextW, MODULEENTRY32W,
    TH32CS_SNAPMODULE, TH32CS_SNAPMODULE32,
};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows::Win32::System::Memory::{
    VirtualAllocEx, VirtualFreeEx, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE,
};
use windows::Win32::System::Threading::{
    CreateRemoteThread, OpenProcess, WaitForSingleObject, PROCESS_ALL_ACCESS,
};
use windows::core::{s, w};

/// Inject a DLL into a target process.
///
/// # Arguments
/// * `pid` - Process ID of the target game
/// * `dll_path` - Absolute path to the DLL file on disk
///
/// # Errors
/// Returns an error if any Win32 API call fails (insufficient privileges,
/// invalid PID, etc.)
pub fn inject_dll(pid: u32, dll_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    // Hard gate: refuse to inject if the DLL is already loaded in the target.
    let dll_filename = std::path::Path::new(dll_path)
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or("omni_overlay.dll");

    if crate::scanner::has_module(pid, dll_filename).unwrap_or(false) {
        info!(pid, dll_filename, "DLL already loaded in target — skipping injection");
        return Ok(());
    }

    // Convert DLL path to wide string (UTF-16) with null terminator
    let wide_path: Vec<u16> = OsStr::new(dll_path)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let path_byte_size = wide_path.len() * std::mem::size_of::<u16>();

    debug!(pid, dll_path, path_byte_size, "Opening target process");

    // Step 1: Open the target process
    let process_handle: HANDLE = unsafe { OpenProcess(PROCESS_ALL_ACCESS, false, pid)? };

    // Wrap in a guard to ensure we always close the handle
    let result = do_injection(process_handle, &wide_path, path_byte_size);

    unsafe {
        let _ = CloseHandle(process_handle);
    }

    result
}

fn do_injection(
    process: HANDLE,
    wide_path: &[u16],
    path_byte_size: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    // Step 2: Allocate memory in the target process for the DLL path
    let remote_mem = unsafe {
        VirtualAllocEx(
            process,
            Some(ptr::null()),
            path_byte_size,
            MEM_COMMIT | MEM_RESERVE,
            PAGE_READWRITE,
        )
    };

    if remote_mem.is_null() {
        return Err("VirtualAllocEx failed — could not allocate memory in target process".into());
    }

    debug!(?remote_mem, "Allocated memory in target process");

    // Step 3: Write the DLL path into the allocated memory
    let write_result = unsafe {
        WriteProcessMemory(
            process,
            remote_mem,
            wide_path.as_ptr() as *const _,
            path_byte_size,
            None,
        )
    };

    if write_result.is_err() {
        unsafe {
            let _ = VirtualFreeEx(process, remote_mem, 0, MEM_RELEASE);
        }
        return Err(format!("WriteProcessMemory failed: {:?}", write_result.err()).into());
    }

    debug!("Wrote DLL path to target process memory");

    // Step 4: Get the address of LoadLibraryW in kernel32.dll
    // kernel32.dll is loaded at the same base address in every process (ASLR applies
    // per-boot, but within a boot session, the address is the same across processes).
    let kernel32 = unsafe { GetModuleHandleW(w!("kernel32.dll"))? };
    let load_library_addr = unsafe { GetProcAddress(kernel32, s!("LoadLibraryW")) };

    let load_library_addr = match load_library_addr {
        Some(addr) => addr,
        None => {
            unsafe {
                let _ = VirtualFreeEx(process, remote_mem, 0, MEM_RELEASE);
            }
            return Err("GetProcAddress failed — could not find LoadLibraryW".into());
        }
    };

    debug!(?load_library_addr, "Found LoadLibraryW address");

    // Step 5: Create a remote thread that calls LoadLibraryW(our_dll_path)
    // SAFETY: load_library_addr is a valid LPTHREAD_START_ROUTINE — LoadLibraryW
    // takes a single LPCWSTR parameter and returns HMODULE (a pointer-sized value).
    let thread_handle = unsafe {
        CreateRemoteThread(
            process,
            None,                                               // default security
            0,                                                  // default stack size
            Some(std::mem::transmute(load_library_addr)),        // LoadLibraryW
            Some(remote_mem),                                   // DLL path as parameter
            0,                                                  // run immediately
            None,                                               // don't need thread ID
        )?
    };

    info!("Created remote thread — waiting for DLL to load");

    // Wait for the remote thread to finish (LoadLibraryW returns)
    unsafe {
        WaitForSingleObject(thread_handle, 10_000); // 10 second timeout
        let _ = CloseHandle(thread_handle);
    }

    // Clean up the allocated memory (LoadLibraryW has already copied the path)
    unsafe {
        let _ = VirtualFreeEx(process, remote_mem, 0, MEM_RELEASE);
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
pub fn eject_dll(pid: u32, dll_name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let module_handle = find_remote_module(pid, dll_name)?
        .ok_or_else(|| format!("Module '{}' not found in pid {}", dll_name, pid))?;

    debug!(pid, dll_name, ?module_handle, "Found module in target process");

    // Find the address of omni_shutdown inside the remote module.
    let shutdown_addr = find_remote_export(pid, dll_name, "omni_shutdown")?
        .ok_or_else(|| format!("'omni_shutdown' export not found in {} (pid {})", dll_name, pid))?;

    debug!(?shutdown_addr, "Found omni_shutdown address");

    let process_handle: HANDLE = unsafe { OpenProcess(PROCESS_ALL_ACCESS, false, pid)? };

    // Create a remote thread that calls omni_shutdown(NULL).
    let thread_handle = unsafe {
        CreateRemoteThread(
            process_handle,
            None,
            0,
            Some(std::mem::transmute(shutdown_addr)),
            None,
            0,
            None,
        )?
    };

    info!("Created remote thread calling omni_shutdown — waiting for clean unload");

    unsafe {
        // omni_shutdown sleeps 200ms then calls FreeLibraryAndExitThread,
        // so 10s is a generous timeout.
        WaitForSingleObject(thread_handle, 10_000);
        let _ = CloseHandle(thread_handle);
        let _ = CloseHandle(process_handle);
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
) -> Result<Option<*const std::ffi::c_void>, Box<dyn std::error::Error>> {
    let remote_base = match find_remote_module(pid, dll_name)? {
        Some(base) => base as usize,
        None => return Ok(None),
    };

    // Find the DLL path from the module entry.
    let dll_path = find_remote_module_path(pid, dll_name)?
        .ok_or_else(|| format!("Could not get path for '{}'", dll_name))?;

    let rva = find_export_rva_from_file(&dll_path, export_name)?
        .ok_or_else(|| format!("Export '{}' not found in '{}'", export_name, dll_path))?;

    let remote_addr = (remote_base + rva as usize) as *const std::ffi::c_void;
    Ok(Some(remote_addr))
}

/// Get the full file path of a module loaded in a remote process.
fn find_remote_module_path(
    pid: u32,
    dll_name: &str,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    let snapshot = unsafe {
        CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid)?
    };

    let mut entry = MODULEENTRY32W {
        dwSize: size_of::<MODULEENTRY32W>() as u32,
        ..Default::default()
    };

    if unsafe { Module32FirstW(snapshot, &mut entry) }.is_err() {
        unsafe { let _ = CloseHandle(snapshot); }
        return Err("Failed to enumerate modules".into());
    }

    loop {
        let name = crate::scanner::wchar_to_string(&entry.szModule);
        if name.eq_ignore_ascii_case(dll_name) {
            let path = crate::scanner::wchar_to_string(&entry.szExePath);
            unsafe { let _ = CloseHandle(snapshot); }
            return Ok(Some(path));
        }

        entry.dwSize = size_of::<MODULEENTRY32W>() as u32;
        match unsafe { Module32NextW(snapshot, &mut entry) } {
            Ok(()) => {}
            Err(e) if e.code() == ERROR_NO_MORE_FILES.to_hresult() => break,
            Err(e) => {
                unsafe { let _ = CloseHandle(snapshot); }
                return Err(e.into());
            }
        }
    }

    unsafe { let _ = CloseHandle(snapshot); }
    Ok(None)
}

/// Parse a PE file on disk and find the RVA of a named export.
fn find_export_rva_from_file(
    dll_path: &str,
    export_name: &str,
) -> Result<Option<u32>, Box<dyn std::error::Error>> {
    let data = std::fs::read(dll_path)?;

    // DOS header: e_lfanew at offset 0x3C.
    if data.len() < 0x40 {
        return Err("File too small for DOS header".into());
    }
    let e_lfanew = u32::from_le_bytes(data[0x3C..0x40].try_into()?) as usize;

    // PE signature + COFF header (20 bytes) + optional header.
    let coff_start = e_lfanew + 4;
    if data.len() < coff_start + 20 {
        return Err("File too small for COFF header".into());
    }

    let optional_hdr_start = coff_start + 20;
    let magic = u16::from_le_bytes(data[optional_hdr_start..optional_hdr_start + 2].try_into()?);

    // Export directory is data directory index 0.
    let export_dir_offset = match magic {
        0x20B => optional_hdr_start + 112, // PE32+ (64-bit)
        0x10B => optional_hdr_start + 96,  // PE32 (32-bit)
        _ => return Err(format!("Unknown PE optional header magic: {:#x}", magic).into()),
    };

    if data.len() < export_dir_offset + 8 {
        return Err("File too small for export data directory".into());
    }

    let export_rva = u32::from_le_bytes(data[export_dir_offset..export_dir_offset + 4].try_into()?) as usize;
    let export_size = u32::from_le_bytes(data[export_dir_offset + 4..export_dir_offset + 8].try_into()?) as usize;

    if export_rva == 0 || export_size == 0 {
        return Ok(None); // No export directory.
    }

    // Convert RVA to file offset using section headers.
    let num_sections = u16::from_le_bytes(data[coff_start + 2..coff_start + 4].try_into()?) as usize;
    let optional_hdr_size = u16::from_le_bytes(data[coff_start + 16..coff_start + 18].try_into()?) as usize;
    let sections_start = optional_hdr_start + optional_hdr_size;

    let rva_to_offset = |rva: usize| -> Option<usize> {
        for i in 0..num_sections {
            let s = sections_start + i * 40;
            let vaddr = u32::from_le_bytes(data[s + 12..s + 16].try_into().ok()?) as usize;
            let vsize = u32::from_le_bytes(data[s + 8..s + 12].try_into().ok()?) as usize;
            let raw_ptr = u32::from_le_bytes(data[s + 20..s + 24].try_into().ok()?) as usize;
            if rva >= vaddr && rva < vaddr + vsize {
                return Some(rva - vaddr + raw_ptr);
            }
        }
        None
    };

    let export_offset = rva_to_offset(export_rva)
        .ok_or("Could not map export directory RVA to file offset")?;

    // Export directory table: NumberOfNames at +24, AddressOfFunctions at +28,
    // AddressOfNames at +32, AddressOfNameOrdinals at +36.
    let num_names = u32::from_le_bytes(data[export_offset + 24..export_offset + 28].try_into()?) as usize;
    let addr_of_functions_rva = u32::from_le_bytes(data[export_offset + 28..export_offset + 32].try_into()?) as usize;
    let addr_of_names_rva = u32::from_le_bytes(data[export_offset + 32..export_offset + 36].try_into()?) as usize;
    let addr_of_ordinals_rva = u32::from_le_bytes(data[export_offset + 36..export_offset + 40].try_into()?) as usize;

    let names_offset = rva_to_offset(addr_of_names_rva)
        .ok_or("Could not map names RVA")?;
    let ordinals_offset = rva_to_offset(addr_of_ordinals_rva)
        .ok_or("Could not map ordinals RVA")?;
    let functions_offset = rva_to_offset(addr_of_functions_rva)
        .ok_or("Could not map functions RVA")?;

    for i in 0..num_names {
        let name_rva = u32::from_le_bytes(data[names_offset + i * 4..names_offset + i * 4 + 4].try_into()?) as usize;
        let name_offset = rva_to_offset(name_rva)
            .ok_or("Could not map export name RVA")?;

        // Read null-terminated name.
        let name_end = data[name_offset..].iter().position(|&b| b == 0)
            .unwrap_or(0) + name_offset;
        let name = std::str::from_utf8(&data[name_offset..name_end])?;

        if name == export_name {
            let ordinal = u16::from_le_bytes(data[ordinals_offset + i * 2..ordinals_offset + i * 2 + 2].try_into()?) as usize;
            let func_rva = u32::from_le_bytes(data[functions_offset + ordinal * 4..functions_offset + ordinal * 4 + 4].try_into()?);
            return Ok(Some(func_rva));
        }
    }

    Ok(None)
}

/// Find a loaded module by name in a remote process, returning its base address.
fn find_remote_module(
    pid: u32,
    dll_name: &str,
) -> Result<Option<*const std::ffi::c_void>, Box<dyn std::error::Error>> {
    let snapshot = unsafe {
        CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid)?
    };

    let mut entry = MODULEENTRY32W {
        dwSize: size_of::<MODULEENTRY32W>() as u32,
        ..Default::default()
    };

    if unsafe { Module32FirstW(snapshot, &mut entry) }.is_err() {
        unsafe { let _ = CloseHandle(snapshot); }
        return Err("Failed to enumerate modules".into());
    }

    loop {
        let name = crate::scanner::wchar_to_string(&entry.szModule);
        if name.eq_ignore_ascii_case(dll_name) {
            let base = entry.modBaseAddr as *const std::ffi::c_void;
            unsafe { let _ = CloseHandle(snapshot); }
            return Ok(Some(base));
        }

        entry.dwSize = size_of::<MODULEENTRY32W>() as u32;
        match unsafe { Module32NextW(snapshot, &mut entry) } {
            Ok(()) => {}
            Err(e) if e.code() == ERROR_NO_MORE_FILES.to_hresult() => break,
            Err(e) => {
                unsafe { let _ = CloseHandle(snapshot); }
                return Err(e.into());
            }
        }
    }

    unsafe { let _ = CloseHandle(snapshot); }
    Ok(None)
}
