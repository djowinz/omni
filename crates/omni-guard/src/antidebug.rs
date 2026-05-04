//! Layered anti-debug checks. Function names are honest after the
//! 2026-05-04 open-sourcing — the private repo's misleading names
//! (`telemetry_tick`, `cache_warmup`, etc.) provided ~no security since
//! the source is public, and hurt maintainer readability.
//!
//! `#[inline(never)]` is kept for a legitimate non-obfuscation reason:
//! prevents the compiler from eliding checks whose return values are
//! consumed by the silent-poison pattern in `device.rs::telemetry_session_seed`.

#[inline(never)]
pub(crate) fn any_check_positive() -> bool {
    is_debugger_present()
        || peb_being_debugged()
        || nt_query_debug_port()
        || hardware_breakpoints_set()
        || rdtsc_single_step_detected()
}

#[inline(never)]
fn is_debugger_present() -> bool {
    use windows::Win32::System::Diagnostics::Debug::IsDebuggerPresent;
    unsafe { IsDebuggerPresent().as_bool() }
}

#[inline(never)]
fn peb_being_debugged() -> bool {
    // PEB->BeingDebugged (byte at PEB+0x02 on x86_64). TEB at gs:[0x60] → PEB.
    #[cfg(target_arch = "x86_64")]
    unsafe {
        use std::arch::asm;
        let peb: u64;
        asm!("mov {peb}, gs:[0x60]", peb = out(reg) peb);
        if peb == 0 {
            false
        } else {
            *((peb + 0x02) as *const u8) != 0 // PEB.BeingDebugged
        }
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        use windows::Win32::System::Diagnostics::Debug::IsDebuggerPresent;
        unsafe { IsDebuggerPresent().as_bool() }
    }
}

#[inline(never)]
fn nt_query_debug_port() -> bool {
    // NtQueryInformationProcess(handle, ProcessDebugPort=7, &port, 8).
    // Non-zero port means a debugger is attached.
    use windows::Wdk::System::Threading::{NtQueryInformationProcess, PROCESSINFOCLASS};
    use windows::Win32::System::Threading::GetCurrentProcess;

    const PROCESS_DEBUG_PORT: PROCESSINFOCLASS = PROCESSINFOCLASS(7);
    let mut port: u64 = 0;
    let status = unsafe {
        NtQueryInformationProcess(
            GetCurrentProcess(),
            PROCESS_DEBUG_PORT,
            &mut port as *mut _ as *mut _,
            8,
            std::ptr::null_mut(),
        )
    };
    status.is_ok() && port != 0
}

#[inline(never)]
fn hardware_breakpoints_set() -> bool {
    // Hardware breakpoints in DR0..DR3 (GetThreadContext + CONTEXT_DEBUG_REGISTERS).
    use windows::Win32::System::Diagnostics::Debug::{
        GetThreadContext, CONTEXT, CONTEXT_DEBUG_REGISTERS_AMD64,
    };
    use windows::Win32::System::Threading::GetCurrentThread;
    unsafe {
        let h = GetCurrentThread();
        let mut ctx: CONTEXT = std::mem::zeroed();
        ctx.ContextFlags = CONTEXT_DEBUG_REGISTERS_AMD64;
        if GetThreadContext(h, &mut ctx).is_err() {
            return false;
        }
        ctx.Dr0 != 0 || ctx.Dr1 != 0 || ctx.Dr2 != 0 || ctx.Dr3 != 0
    }
}

#[inline(never)]
fn rdtsc_single_step_detected() -> bool {
    // RDTSC timing: measure a fixed tight loop; >200k cycles for 1024 muls
    // suggests single-step/trace instrumentation.
    use std::arch::x86_64::_rdtsc;
    let start = unsafe { _rdtsc() };
    let mut acc: u64 = 1;
    for i in 0..1024u64 {
        acc = acc.wrapping_mul(i | 1);
    }
    std::hint::black_box(acc);
    let end = unsafe { _rdtsc() };
    end.wrapping_sub(start) > 200_000
}
