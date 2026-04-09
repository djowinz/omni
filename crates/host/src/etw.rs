//! ETW-based DXGI frame capture for external overlay mode.
//!
//! Uses the `Microsoft-Windows-DXGI` ETW provider to capture Present events
//! from a target process without injection. Computes FPS and frame-time
//! percentile metrics from the inter-frame deltas.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use windows::core::{GUID, PCWSTR};
use windows::Win32::Foundation::WIN32_ERROR;
use windows::Win32::System::Diagnostics::Etw::{
    ControlTraceW, EnableTraceEx2, OpenTraceW, ProcessTrace, StartTraceW, CONTROLTRACE_HANDLE,
    EVENT_RECORD, EVENT_TRACE_CONTROL_STOP, EVENT_TRACE_LOGFILEW, EVENT_TRACE_PROPERTIES,
    EVENT_TRACE_REAL_TIME_MODE, PROCESS_TRACE_MODE_EVENT_RECORD, PROCESS_TRACE_MODE_REAL_TIME,
    WNODE_FLAG_TRACED_GUID,
};
use windows::Win32::System::Performance::QueryPerformanceFrequency;

/// Wrapper to send a raw pointer across thread boundaries.
/// Safety: the pointed-to data is only accessed from the consumer thread.
struct SendPtr(*mut CallbackState);
unsafe impl Send for SendPtr {}

impl SendPtr {
    /// Extract the raw pointer. Callers must uphold all aliasing rules.
    unsafe fn as_raw(&self) -> *mut CallbackState {
        self.0
    }
}

/// Microsoft-Windows-DXGI provider GUID.
const DXGI_PROVIDER: GUID = GUID::from_u128(0xCA11C036_0102_4A2D_A6AD_F03CFED5D3C9);

const EVENT_CONTROL_CODE_ENABLE: u32 = 1;

const TRACE_LEVEL_INFORMATION: u8 = 4;

/// Ring buffer capacity — ~5 seconds at 60fps.
const RING_CAPACITY: usize = 300;

/// How many frames between metric recomputation.
const RECOMPUTE_INTERVAL: usize = 30;

/// Frame-timing metrics derived from ETW Present events.
#[derive(Clone, Debug)]
pub struct EtwFrameMetrics {
    pub fps: f32,
    pub frame_time_ms: f32,
    pub frame_time_avg_ms: f32,
    pub frame_time_1pct_ms: f32,
    pub frame_time_01pct_ms: f32,
    pub available: bool,
}

impl From<EtwFrameMetrics> for omni_shared::FrameData {
    fn from(m: EtwFrameMetrics) -> Self {
        Self {
            fps: m.fps,
            frame_time_ms: m.frame_time_ms,
            frame_time_avg_ms: m.frame_time_avg_ms,
            frame_time_1percent_ms: m.frame_time_1pct_ms,
            frame_time_01percent_ms: m.frame_time_01pct_ms,
            available: m.available,
            ..Default::default()
        }
    }
}

impl Default for EtwFrameMetrics {
    fn default() -> Self {
        Self {
            fps: 0.0,
            frame_time_ms: 0.0,
            frame_time_avg_ms: 0.0,
            frame_time_1pct_ms: 0.0,
            frame_time_01pct_ms: 0.0,
            available: false,
        }
    }
}

/// Shared state passed to the ETW callback via the `Context` pointer.
struct CallbackState {
    target_pid: u32,
    last_present_qpc: Option<i64>,
    qpc_frequency: f64,
    ring: Vec<f32>,
    ring_pos: usize,
    ring_count: usize,
    frames_since_recompute: usize,
    sorted_scratch: Vec<f32>,
    metrics: Arc<Mutex<EtwFrameMetrics>>,
}

/// Handle to a running ETW capture session.
pub struct EtwCapture {
    running: Arc<AtomicBool>,
    /// Owns the callback state Box and the consumer thread. Dropped after the
    /// watchdog has stopped the ETW session, ensuring the consumer thread has
    /// exited before the Box is freed.
    consumer_guard: Option<ConsumerGuard>,
    watchdog_thread: Option<std::thread::JoinHandle<()>>,
    metrics: Arc<Mutex<EtwFrameMetrics>>,
}

impl EtwCapture {
    /// Start capturing DXGI Present events for `target_pid`.
    ///
    /// Requires administrator privileges. Returns an error string on failure.
    pub fn start(target_pid: u32) -> Result<Self, String> {
        let running = Arc::new(AtomicBool::new(true));
        let metrics = Arc::new(Mutex::new(EtwFrameMetrics::default()));

        // ── 1. Build the session name as a wide string ──────────────────
        let session_name = format!("OmniDxgiCapture_{}", target_pid);
        let session_wide: Vec<u16> = session_name
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();

        // ── 2. Allocate EVENT_TRACE_PROPERTIES + trailing session name ──
        let props_size = std::mem::size_of::<EVENT_TRACE_PROPERTIES>();
        let name_bytes = session_wide.len() * 2; // u16 → bytes
        let total = props_size + name_bytes;

        let mut buf = vec![0u8; total];
        let props = buf.as_mut_ptr() as *mut EVENT_TRACE_PROPERTIES;

        unsafe {
            (*props).Wnode.BufferSize = total as u32;
            (*props).Wnode.Flags = WNODE_FLAG_TRACED_GUID;
            (*props).Wnode.ClientContext = 1; // QPC clock resolution
            (*props).LogFileMode = EVENT_TRACE_REAL_TIME_MODE;
            (*props).LoggerNameOffset = props_size as u32;

            // Copy session name after the struct.
            std::ptr::copy_nonoverlapping(
                session_wide.as_ptr() as *const u8,
                buf.as_mut_ptr().add(props_size),
                name_bytes,
            );
        }

        // ── 3. Start the trace session ──────────────────────────────────
        let mut session_handle = CONTROLTRACE_HANDLE { Value: 0 };

        let mut err =
            unsafe { StartTraceW(&mut session_handle, PCWSTR(session_wide.as_ptr()), props) };

        // If a stale session exists, stop it and retry.
        if err == WIN32_ERROR(0xB7) {
            // ERROR_ALREADY_EXISTS
            unsafe {
                let err = ControlTraceW(
                    CONTROLTRACE_HANDLE { Value: 0 },
                    PCWSTR(session_wide.as_ptr()),
                    props,
                    EVENT_TRACE_CONTROL_STOP,
                );
                if err.0 != 0 {
                    tracing::debug!(
                        "ControlTraceW (stop stale session) returned 0x{:08X}",
                        err.0
                    );
                }
            }
            // Re-zero the buffer and rebuild properties.
            buf.fill(0);
            unsafe {
                (*props).Wnode.BufferSize = total as u32;
                (*props).Wnode.Flags = WNODE_FLAG_TRACED_GUID;
                (*props).Wnode.ClientContext = 1;
                (*props).LogFileMode = EVENT_TRACE_REAL_TIME_MODE;
                (*props).LoggerNameOffset = props_size as u32;
                std::ptr::copy_nonoverlapping(
                    session_wide.as_ptr() as *const u8,
                    buf.as_mut_ptr().add(props_size),
                    name_bytes,
                );
            }
            err = unsafe { StartTraceW(&mut session_handle, PCWSTR(session_wide.as_ptr()), props) };
        }

        if err.0 != 0 {
            return Err(format!("StartTraceW failed: 0x{:08X}", err.0));
        }

        // ── 4. Enable the DXGI provider ─────────────────────────────────
        let err = unsafe {
            EnableTraceEx2(
                session_handle,
                &DXGI_PROVIDER,
                EVENT_CONTROL_CODE_ENABLE,
                TRACE_LEVEL_INFORMATION,
                0xFFFFFFFFFFFFFFFF, // match any keyword
                0,
                0,
                None,
            )
        };
        if err.0 != 0 {
            // Clean up session on failure.
            unsafe {
                let stop_err = ControlTraceW(
                    session_handle,
                    PCWSTR::null(),
                    props,
                    EVENT_TRACE_CONTROL_STOP,
                );
                if stop_err.0 != 0 {
                    tracing::debug!(
                        "ControlTraceW (cleanup after EnableTraceEx2 failure) returned 0x{:08X}",
                        stop_err.0
                    );
                }
            }
            return Err(format!("EnableTraceEx2 failed: 0x{:08X}", err.0));
        }

        // ── 5. Query QPC frequency and prepare callback state ──────────
        let qpc_frequency = {
            let mut freq = 0i64;
            unsafe {
                QueryPerformanceFrequency(&mut freq)
                    .map_err(|e| format!("QueryPerformanceFrequency failed: {e}"))?;
            }
            freq as f64
        };

        let state = Box::new(CallbackState {
            target_pid,
            last_present_qpc: None,
            qpc_frequency,
            ring: vec![0.0; RING_CAPACITY],
            ring_pos: 0,
            ring_count: 0,
            frames_since_recompute: 0,
            sorted_scratch: Vec::with_capacity(RING_CAPACITY),
            metrics: Arc::clone(&metrics),
        });
        let state_ptr = Box::into_raw(state);

        // ── 6. Spawn consumer thread ────────────────────────────────────
        let session_name_for_consumer: Vec<u16> = session_wide.clone();
        let send_ptr = SendPtr(state_ptr);

        let consumer_thread = std::thread::Builder::new()
            .name("etw-consumer".into())
            .spawn(move || {
                unsafe {
                    let raw = send_ptr.as_raw();
                    // Build the logfile descriptor.
                    let mut logfile: EVENT_TRACE_LOGFILEW = std::mem::zeroed();
                    logfile.LoggerName =
                        windows::core::PWSTR(session_name_for_consumer.as_ptr() as *mut u16);
                    logfile.Anonymous1.ProcessTraceMode =
                        PROCESS_TRACE_MODE_REAL_TIME | PROCESS_TRACE_MODE_EVENT_RECORD;
                    logfile.Anonymous2.EventRecordCallback = Some(event_record_callback);
                    logfile.Context = raw as *mut core::ffi::c_void;

                    let trace_handle = OpenTraceW(&mut logfile);
                    if trace_handle.Value == u64::MAX {
                        tracing::error!("OpenTraceW failed");
                        return;
                    }

                    // ProcessTrace blocks until the session is stopped.
                    let err = ProcessTrace(&[trace_handle], None, None);
                    if err.0 != 0 && err.0 != 0x0000_0103
                    /* ERROR_NO_MORE_ITEMS */
                    {
                        tracing::warn!("ProcessTrace exited: 0x{:08X}", err.0);
                    }
                }
            })
            .map_err(|e| format!("Failed to spawn consumer thread: {e}"))?;

        // ── 7. Spawn watchdog thread ────────────────────────────────────
        let running_watchdog = Arc::clone(&running);
        // We need to move the props buffer and session name into the
        // watchdog so it can stop the trace.
        let watchdog_buf = buf;
        let watchdog_session_name = session_wide;

        let watchdog_thread = std::thread::Builder::new()
            .name("etw-watchdog".into())
            .spawn(move || {
                while running_watchdog.load(Ordering::Relaxed) {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
                // Stop the session so ProcessTrace returns.
                let mut stop_buf = watchdog_buf;
                let stop_props = stop_buf.as_mut_ptr() as *mut EVENT_TRACE_PROPERTIES;
                unsafe {
                    let err = ControlTraceW(
                        CONTROLTRACE_HANDLE { Value: 0 },
                        PCWSTR(watchdog_session_name.as_ptr()),
                        stop_props,
                        EVENT_TRACE_CONTROL_STOP,
                    );
                    if err.0 != 0 {
                        tracing::debug!("ControlTraceW (watchdog stop) returned 0x{:08X}", err.0);
                    }
                }
            })
            .map_err(|e| format!("Failed to spawn watchdog thread: {e}"))?;

        // ConsumerGuard owns the state Box and the consumer thread handle.
        // Its Drop joins the consumer thread before freeing the Box, preventing
        // any use-after-free if the thread is still inside event_record_callback.
        let consumer_guard = ConsumerGuard {
            state_ptr: SendPtr(state_ptr),
            running: Arc::clone(&running),
            consumer_thread: Some(consumer_thread),
        };

        Ok(Self {
            running,
            consumer_guard: Some(consumer_guard),
            watchdog_thread: Some(watchdog_thread),
            metrics,
        })
    }

    /// Return the most recently computed metrics.
    pub fn latest_metrics(&self) -> EtwFrameMetrics {
        self.metrics.lock().unwrap().clone()
    }

    /// Signal the capture to stop and wait for threads to exit.
    pub fn stop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Ok(mut m) = self.metrics.lock() {
            m.available = false;
        }
        // Join the watchdog first; it will stop the ETW session, causing
        // ProcessTrace to return in the consumer thread.
        if let Some(h) = self.watchdog_thread.take() {
            if let Err(e) = h.join() {
                tracing::warn!("ETW watchdog thread panicked: {e:?}");
            }
        }
        // Dropping ConsumerGuard joins the consumer thread and then frees the
        // callback state Box — in that order, preventing use-after-free.
        drop(self.consumer_guard.take());
    }
}

impl Drop for EtwCapture {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Guard that owns the callback state Box and the consumer thread handle.
///
/// Drop order is guaranteed:
/// 1. Signal the consumer thread to stop.
/// 2. Join the consumer thread (wait for ProcessTrace and any in-flight
///    `event_record_callback` invocations to complete).
/// 3. Free the callback state Box — safe because the thread has exited.
struct ConsumerGuard {
    state_ptr: SendPtr,
    running: Arc<AtomicBool>,
    consumer_thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for ConsumerGuard {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Release);
        // Wait for the consumer thread to exit before freeing the state Box.
        if let Some(handle) = self.consumer_thread.take() {
            if let Err(e) = handle.join() {
                tracing::warn!("ETW consumer thread panicked: {e:?}");
            }
        }
        // NOW safe to free the state — no thread can be inside the callback.
        unsafe {
            drop(Box::from_raw(self.state_ptr.as_raw()));
        }
    }
}

/// ETW event record callback — called on the consumer thread for every event.
unsafe extern "system" fn event_record_callback(record: *mut EVENT_RECORD) {
    let record = &*record;
    let ctx = record.UserContext as *mut CallbackState;
    if ctx.is_null() {
        return;
    }
    let state = &mut *ctx;

    // Filter to our target process.
    if record.EventHeader.ProcessId != state.target_pid {
        return;
    }

    // Filter to Present completion events (opcode 2 = Stop).
    // DX11 games emit ID 181 (Present::Stop).
    // DX12 games emit ID 43 (PresentMultiplaneOverlay::Stop).
    // Some games emit both — deduplicate by ignoring events within 0.5ms
    // of the previous event (two present completions < 0.5ms apart are
    // the same frame reported twice, not two separate frames).
    let id = record.EventHeader.EventDescriptor.Id;
    let opcode = record.EventHeader.EventDescriptor.Opcode;
    if opcode != 2 || (id != 181 && id != 43) {
        return;
    }

    let qpc_now = record.EventHeader.TimeStamp;

    // Deduplicate: skip if this event is within 0.5ms of the last one
    if let Some(prev_qpc) = state.last_present_qpc {
        let delta_ticks = qpc_now - prev_qpc;
        let delta_ms = (delta_ticks as f64 / state.qpc_frequency) * 1000.0;
        if delta_ms < 0.5 {
            return; // Same frame, different event ID — skip
        }
    }
    if let Some(prev_qpc) = state.last_present_qpc {
        let delta_ticks = qpc_now - prev_qpc;
        let delta_ms = (delta_ticks as f64 / state.qpc_frequency) * 1000.0;
        let delta_ms = delta_ms as f32;

        // Push into ring buffer.
        state.ring[state.ring_pos] = delta_ms;
        state.ring_pos = (state.ring_pos + 1) % RING_CAPACITY;
        if state.ring_count < RING_CAPACITY {
            state.ring_count += 1;
        }

        // Update instantaneous frame time.
        if let Ok(mut m) = state.metrics.lock() {
            m.frame_time_ms = delta_ms;
            m.available = true;
        }

        state.frames_since_recompute += 1;
        if state.frames_since_recompute >= RECOMPUTE_INTERVAL && state.ring_count >= 2 {
            state.frames_since_recompute = 0;
            recompute_metrics(state);
        }
    }
    state.last_present_qpc = Some(qpc_now);
}

/// Recompute aggregate metrics from the ring buffer.
fn recompute_metrics(state: &mut CallbackState) {
    let count = state.ring_count;
    state.sorted_scratch.clear();
    state.sorted_scratch.extend_from_slice(&state.ring[..count]);
    state
        .sorted_scratch
        .sort_unstable_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let sum: f32 = state.sorted_scratch.iter().sum();
    let avg = sum / count as f32;
    let fps = if avg > 0.0 { 1000.0 / avg } else { 0.0 };

    // 1% high = index at 99th percentile of frame times (= 1% low FPS).
    let idx_1pct = ((count as f32 * 0.99) as usize).min(count - 1);
    // 0.1% high = index at 99.9th percentile.
    let idx_01pct = ((count as f32 * 0.999) as usize).min(count - 1);

    if let Ok(mut m) = state.metrics.lock() {
        m.fps = fps;
        m.frame_time_avg_ms = avg;
        m.frame_time_1pct_ms = state.sorted_scratch[idx_1pct];
        m.frame_time_01pct_ms = state.sorted_scratch[idx_01pct];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_metrics_not_available() {
        let m = EtwFrameMetrics::default();
        assert!(!m.available);
        assert_eq!(m.fps, 0.0);
    }

    #[test]
    fn recompute_produces_sane_values() {
        let metrics = Arc::new(Mutex::new(EtwFrameMetrics::default()));
        let mut state = CallbackState {
            target_pid: 0,
            last_present_qpc: None,
            qpc_frequency: 10_000_000.0,
            ring: vec![0.0; RING_CAPACITY],
            ring_pos: 0,
            ring_count: 0,
            frames_since_recompute: 0,
            sorted_scratch: Vec::with_capacity(RING_CAPACITY),
            metrics: Arc::clone(&metrics),
        };
        // Simulate 60fps (16.67ms frames).
        for i in 0..100 {
            state.ring[i] = 16.67;
            state.ring_count = i + 1;
            state.ring_pos = i + 1;
        }
        recompute_metrics(&mut state);
        let m = metrics.lock().unwrap();
        assert!(m.fps > 59.0 && m.fps < 61.0, "fps={}", m.fps);
        assert!(
            (m.frame_time_avg_ms - 16.67).abs() < 0.1,
            "avg={}",
            m.frame_time_avg_ms
        );
    }
}
