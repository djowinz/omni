# Phase 8: Frame Timing via Present Hook

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Display real-time FPS in the game overlay by measuring frame times directly in the DLL's Present hook using `QueryPerformanceCounter`.

**Architecture:** A self-contained `FrameStats` module in the DLL records high-resolution timestamps on each Present call, maintains a ring buffer of the last 1000 frame times, and computes FPS, frame time, rolling average, and 1%/0.1% lows. The DLL overrides the host's "FPS: N/A" widget text with its computed values before rendering. No ETW, no admin privileges, no host-side changes.

**Tech Stack:** Rust, `QueryPerformanceCounter` / `QueryPerformanceFrequency` (Windows high-resolution timer), `windows` crate 0.58.

**Testing notes:** `FrameStats` is fully unit-testable with synthetic timestamps. FPS accuracy validated manually by comparing with the NVIDIA overlay.

**Depends on:** Phases 1–7 complete (DLL hooks, D2D renderer, shared memory, widget display all working).

---

## File Map

```
overlay-dll/
  Cargo.toml                         # Add Win32_System_Performance feature
  src/
    lib.rs                           # Add mod frame_stats;
    frame_stats.rs                   # NEW: ring buffer, QPC timing, FPS/percentile computation
    present.rs                       # Call frame_stats.record(), override FPS widget text
```

---

### Task 1: Add Performance Counter Windows Feature

**Files:**
- Modify: `overlay-dll/Cargo.toml`

- [ ] **Step 1: Add Win32_System_Performance to windows features**

Add `"Win32_System_Performance"` to the features list in `overlay-dll/Cargo.toml`:

```toml
[dependencies.windows]
version = "0.58"
features = [
    "Win32_Foundation",
    "Win32_System_SystemServices",
    "Win32_System_LibraryLoader",
    "Win32_Graphics_Direct3D",
    "Win32_Graphics_Direct3D11",
    "Win32_Graphics_Direct3D11on12",
    "Win32_Graphics_Direct3D12",
    "Win32_Graphics_Dxgi",
    "Win32_Graphics_Dxgi_Common",
    "Win32_Graphics_Gdi",
    "Win32_UI_WindowsAndMessaging",
    "Win32_Graphics_Direct3D_Fxc",
    "Win32_Graphics_Direct2D",
    "Win32_Graphics_Direct2D_Common",
    "Win32_Graphics_DirectWrite",
    "Win32_System_Memory",
    "Win32_System_Performance",
    "Foundation_Numerics",
]
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check -p omni-overlay-dll`
Expected: Compiles successfully.

- [ ] **Step 3: Commit**

```bash
git add overlay-dll/Cargo.toml Cargo.lock
git commit -m "feat(overlay-dll): add Win32_System_Performance for QueryPerformanceCounter"
```

---

### Task 2: FrameStats Module

**Files:**
- Create: `overlay-dll/src/frame_stats.rs`
- Modify: `overlay-dll/src/lib.rs` (add `mod frame_stats;`)

- [ ] **Step 1: Create overlay-dll/src/frame_stats.rs**

```rust
//! Frame timing statistics computed from Present hook timestamps.
//!
//! Records QueryPerformanceCounter values on each Present call,
//! maintains a ring buffer of frame times, and computes FPS,
//! frame time, rolling average, and 1%/0.1% low percentiles.

use windows::Win32::System::Performance::{
    QueryPerformanceCounter, QueryPerformanceFrequency,
};

const RING_BUFFER_SIZE: usize = 1000;
const FPS_SMOOTHING_FRAMES: usize = 100;
const PERCENTILE_RECALC_INTERVAL: usize = 100;

pub struct FrameStats {
    /// Ring buffer of frame times in milliseconds.
    ring: [f32; RING_BUFFER_SIZE],
    /// Next write position in the ring buffer.
    head: usize,
    /// Number of frames recorded (capped at RING_BUFFER_SIZE).
    count: usize,
    /// Total frames recorded (never wraps — used for percentile recalc interval).
    total_frames: u64,
    /// Previous QueryPerformanceCounter value.
    last_qpc: i64,
    /// Cached QPC frequency (ticks per second), converted to f64.
    qpc_freq: f64,
    /// Cached percentile values (recalculated every PERCENTILE_RECALC_INTERVAL frames).
    cached_1pct_ms: f32,
    cached_01pct_ms: f32,
}

impl FrameStats {
    /// Create a new FrameStats. Call once during renderer init.
    pub fn new() -> Self {
        let mut freq: i64 = 0;
        unsafe {
            let _ = QueryPerformanceFrequency(&mut freq);
        }

        let mut qpc: i64 = 0;
        unsafe {
            let _ = QueryPerformanceCounter(&mut qpc);
        }

        Self {
            ring: [0.0; RING_BUFFER_SIZE],
            head: 0,
            count: 0,
            total_frames: 0,
            last_qpc: qpc,
            qpc_freq: freq as f64,
            cached_1pct_ms: 0.0,
            cached_01pct_ms: 0.0,
        }
    }

    /// Create with explicit values (for testing without QPC).
    #[cfg(test)]
    pub fn new_with_freq(freq: f64) -> Self {
        Self {
            ring: [0.0; RING_BUFFER_SIZE],
            head: 0,
            count: 0,
            total_frames: 0,
            last_qpc: 0,
            qpc_freq: freq,
            cached_1pct_ms: 0.0,
            cached_01pct_ms: 0.0,
        }
    }

    /// Record a frame. Call once per Present hook invocation.
    pub fn record(&mut self) {
        let mut qpc: i64 = 0;
        unsafe {
            let _ = QueryPerformanceCounter(&mut qpc);
        }

        if self.last_qpc > 0 && self.qpc_freq > 0.0 {
            let delta_ticks = (qpc - self.last_qpc) as f64;
            let frame_time_ms = (delta_ticks / self.qpc_freq) * 1000.0;

            // Reject absurd values (> 1 second or negative)
            if frame_time_ms > 0.0 && frame_time_ms < 1000.0 {
                self.ring[self.head] = frame_time_ms as f32;
                self.head = (self.head + 1) % RING_BUFFER_SIZE;
                if self.count < RING_BUFFER_SIZE {
                    self.count += 1;
                }
                self.total_frames += 1;

                // Recalculate percentiles periodically
                if self.total_frames % PERCENTILE_RECALC_INTERVAL as u64 == 0 && self.count >= 10 {
                    self.recalc_percentiles();
                }
            }
        }

        self.last_qpc = qpc;
    }

    /// Record a frame with an explicit timestamp (for testing).
    #[cfg(test)]
    pub fn record_with_qpc(&mut self, qpc: i64) {
        if self.last_qpc > 0 && self.qpc_freq > 0.0 {
            let delta_ticks = (qpc - self.last_qpc) as f64;
            let frame_time_ms = (delta_ticks / self.qpc_freq) * 1000.0;

            if frame_time_ms > 0.0 && frame_time_ms < 1000.0 {
                self.ring[self.head] = frame_time_ms as f32;
                self.head = (self.head + 1) % RING_BUFFER_SIZE;
                if self.count < RING_BUFFER_SIZE {
                    self.count += 1;
                }
                self.total_frames += 1;

                if self.total_frames % PERCENTILE_RECALC_INTERVAL as u64 == 0 && self.count >= 10 {
                    self.recalc_percentiles();
                }
            }
        }

        self.last_qpc = qpc;
    }

    /// Current FPS — rolling average over the last FPS_SMOOTHING_FRAMES frames.
    pub fn fps(&self) -> f32 {
        if self.count == 0 {
            return 0.0;
        }

        let window = self.count.min(FPS_SMOOTHING_FRAMES);
        let mut sum = 0.0f64;

        for i in 0..window {
            let idx = (self.head + RING_BUFFER_SIZE - 1 - i) % RING_BUFFER_SIZE;
            sum += self.ring[idx] as f64;
        }

        if sum > 0.0 {
            (window as f64 / (sum / 1000.0)) as f32
        } else {
            0.0
        }
    }

    /// Latest frame time in milliseconds.
    pub fn frame_time_ms(&self) -> f32 {
        if self.count == 0 {
            return 0.0;
        }
        let idx = (self.head + RING_BUFFER_SIZE - 1) % RING_BUFFER_SIZE;
        self.ring[idx]
    }

    /// Average frame time over the entire ring buffer.
    pub fn frame_time_avg_ms(&self) -> f32 {
        if self.count == 0 {
            return 0.0;
        }

        let mut sum = 0.0f64;
        for i in 0..self.count {
            sum += self.ring[i] as f64;
        }
        (sum / self.count as f64) as f32
    }

    /// 1% low frame time (99th percentile — the worst 1% of frames).
    pub fn frame_time_1pct_ms(&self) -> f32 {
        self.cached_1pct_ms
    }

    /// 0.1% low frame time (99.9th percentile — the worst 0.1% of frames).
    pub fn frame_time_01pct_ms(&self) -> f32 {
        self.cached_01pct_ms
    }

    /// Whether we have enough data to display stats.
    pub fn available(&self) -> bool {
        self.count >= 10
    }

    /// Recalculate percentile values from the ring buffer.
    fn recalc_percentiles(&mut self) {
        let mut sorted = Vec::with_capacity(self.count);
        for i in 0..self.count {
            sorted.push(self.ring[i]);
        }
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        // 1% low = value at the 99th percentile index
        let idx_1pct = ((self.count as f64) * 0.99).floor() as usize;
        self.cached_1pct_ms = sorted[idx_1pct.min(sorted.len() - 1)];

        // 0.1% low = value at the 99.9th percentile index
        let idx_01pct = ((self.count as f64) * 0.999).floor() as usize;
        self.cached_01pct_ms = sorted[idx_01pct.min(sorted.len() - 1)];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_stats_with_frames(frame_times_ms: &[f32]) -> FrameStats {
        // Use freq = 1000 so 1 tick = 1ms
        let mut stats = FrameStats::new_with_freq(1000.0);
        let mut qpc = 1000i64; // start at some non-zero value
        stats.last_qpc = qpc;

        for &ft in frame_times_ms {
            qpc += (ft * 1.0) as i64; // 1 tick per ms at freq=1000
            stats.record_with_qpc(qpc);
        }

        stats
    }

    #[test]
    fn fps_from_known_frame_times() {
        // 100 frames at 16.67ms each = 60 FPS
        let frames: Vec<f32> = vec![16.67; 100];
        let stats = make_stats_with_frames(&frames);
        let fps = stats.fps();
        assert!((fps - 60.0).abs() < 1.0, "Expected ~60 FPS, got {fps}");
    }

    #[test]
    fn fps_from_high_framerate() {
        // 100 frames at 6.94ms each = ~144 FPS
        let frames: Vec<f32> = vec![6.94; 100];
        let stats = make_stats_with_frames(&frames);
        let fps = stats.fps();
        assert!((fps - 144.0).abs() < 2.0, "Expected ~144 FPS, got {fps}");
    }

    #[test]
    fn frame_time_latest() {
        let stats = make_stats_with_frames(&[16.0, 17.0, 15.0, 20.0]);
        assert!((stats.frame_time_ms() - 20.0).abs() < 0.1,
            "Latest frame time should be ~20ms, got {}", stats.frame_time_ms());
    }

    #[test]
    fn frame_time_average() {
        let stats = make_stats_with_frames(&[10.0, 20.0, 30.0, 40.0]);
        let avg = stats.frame_time_avg_ms();
        assert!((avg - 25.0).abs() < 0.1, "Average should be 25ms, got {avg}");
    }

    #[test]
    fn ring_buffer_wraps_correctly() {
        // Fill more than RING_BUFFER_SIZE frames
        let frames: Vec<f32> = (0..1200).map(|_| 16.67).collect();
        let stats = make_stats_with_frames(&frames);
        assert_eq!(stats.count, RING_BUFFER_SIZE);
        let fps = stats.fps();
        assert!((fps - 60.0).abs() < 1.0, "FPS should be ~60 after wrap, got {fps}");
    }

    #[test]
    fn percentiles_computed() {
        // 100 frames to trigger recalc: 90 fast (10ms) + 10 slow (50ms)
        let mut frames: Vec<f32> = vec![10.0; 90];
        frames.extend(vec![50.0; 10]);
        let stats = make_stats_with_frames(&frames);
        // 1% low should be the slow frames (~50ms)
        assert!(stats.frame_time_1pct_ms() >= 40.0,
            "1% low should be ≥40ms, got {}", stats.frame_time_1pct_ms());
    }

    #[test]
    fn available_needs_minimum_frames() {
        let mut stats = FrameStats::new_with_freq(1000.0);
        stats.last_qpc = 1000;
        assert!(!stats.available(), "Should not be available with 0 frames");

        for i in 1..=10 {
            stats.record_with_qpc(1000 + i * 16);
        }
        assert!(stats.available(), "Should be available after 10 frames");
    }

    #[test]
    fn empty_stats_return_zero() {
        let stats = FrameStats::new_with_freq(1000.0);
        assert_eq!(stats.fps(), 0.0);
        assert_eq!(stats.frame_time_ms(), 0.0);
        assert_eq!(stats.frame_time_avg_ms(), 0.0);
        assert!(!stats.available());
    }
}
```

- [ ] **Step 2: Add mod declaration to lib.rs**

Add `mod frame_stats;` after the existing module declarations in `overlay-dll/src/lib.rs`:

```rust
mod logging;
mod hook;
mod present;
mod renderer;
mod ipc;
mod frame_stats;
```

- [ ] **Step 3: Verify it compiles and tests pass**

Run: `cargo test -p omni-overlay-dll`
Expected: 9 tests pass (1 existing vtable test + 8 new frame_stats tests).

- [ ] **Step 4: Commit**

```bash
git add overlay-dll/src/frame_stats.rs overlay-dll/src/lib.rs
git commit -m "feat(overlay-dll): add FrameStats module with ring buffer and percentile computation"
```

---

### Task 3: Wire FrameStats into Present Hook + Override FPS Widget

**Files:**
- Modify: `overlay-dll/src/present.rs`

- [ ] **Step 1: Add FrameStats global and initialization**

Add the following after the existing `static mut SHM_READER` line in `present.rs`:

```rust
static mut FRAME_STATS: Option<crate::frame_stats::FrameStats> = None;
```

Update `ensure_renderer` to also initialize `FRAME_STATS`:

After the line `RENDERER_INIT_DONE.store(true, Ordering::Release);` (in the `Ok` branch), add:

```rust
                FRAME_STATS = Some(crate::frame_stats::FrameStats::new());
                log_to_file("[present] frame stats initialized");
```

Update `destroy_renderer` to also clean up `FRAME_STATS`:

After the existing `SHM_READER.take()` block, add:

```rust
    FRAME_STATS = None;
    log_to_file("[present] frame stats destroyed");
```

- [ ] **Step 2: Record frame timing and override FPS widget text**

Replace the existing `render_overlay` function with:

```rust
/// Common rendering logic shared by hooked_present and hooked_present1.
unsafe fn render_overlay(swap_chain: *mut c_void) {
    ensure_renderer();
    ensure_shm_reader();

    // Record frame timing
    if let Some(frame_stats) = &mut FRAME_STATS {
        frame_stats.record();
    }

    let renderer = match &mut RENDERER {
        Some(r) => r,
        None => return,
    };

    // Read widgets from shared memory
    let widgets = match &mut SHM_READER {
        Some(reader) => {
            let slot = reader.read_current();
            let count = slot.widget_count as usize;
            if count > 0 {
                &slot.widgets[..count]
            } else {
                return;
            }
        }
        None => return,
    };

    // Copy widgets to a local buffer so we can override FPS text
    let mut local_widgets: Vec<omni_shared::ComputedWidget> = widgets.to_vec();

    // Override frame timing widget text with DLL-computed values
    if let Some(frame_stats) = &FRAME_STATS {
        if frame_stats.available() {
            for widget in &mut local_widgets {
                match widget.source {
                    omni_shared::SensorSource::Fps => {
                        let text = format!("FPS: {:.0}", frame_stats.fps());
                        omni_shared::write_fixed_str(&mut widget.format_pattern, &text);
                    }
                    omni_shared::SensorSource::FrameTime => {
                        let text = format!("Frame: {:.1}ms", frame_stats.frame_time_ms());
                        omni_shared::write_fixed_str(&mut widget.format_pattern, &text);
                    }
                    omni_shared::SensorSource::FrameTimeAvg => {
                        let text = format!("Avg: {:.1}ms", frame_stats.frame_time_avg_ms());
                        omni_shared::write_fixed_str(&mut widget.format_pattern, &text);
                    }
                    omni_shared::SensorSource::FrameTime1Pct => {
                        let text = format!("1%: {:.1}ms", frame_stats.frame_time_1pct_ms());
                        omni_shared::write_fixed_str(&mut widget.format_pattern, &text);
                    }
                    omni_shared::SensorSource::FrameTime01Pct => {
                        let text = format!("0.1%: {:.1}ms", frame_stats.frame_time_01pct_ms());
                        omni_shared::write_fixed_str(&mut widget.format_pattern, &text);
                    }
                    _ => {}
                }
            }
        }
    }

    renderer.render(swap_chain, &local_widgets);
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check -p omni-overlay-dll`
Expected: Compiles successfully. Warnings about static_mut_refs are pre-existing.

- [ ] **Step 4: Run all tests**

Run: `cargo test`
Expected: All tests pass across all crates.

- [ ] **Step 5: Commit**

```bash
git add overlay-dll/src/present.rs
git commit -m "feat(overlay-dll): wire FrameStats into Present hook, override FPS widget text"
```

---

### Task 4: Integration Test — FPS Counter in Game

This is a manual integration test.

- [ ] **Step 1: Build everything**

```bash
cargo build -p omni-host && cargo build -p omni-overlay-dll
```

- [ ] **Step 2: Start host and launch a game**

```bash
cargo run -p omni-host -- --watch target/debug/omni_overlay_dll.dll
```

Launch a DX11 or DX12 game.

- [ ] **Step 3: Verify FPS counter**

The overlay should now show a real FPS number instead of "FPS: N/A". Compare with the NVIDIA overlay's FPS counter — they should be close (within 1-2 FPS).

- [ ] **Step 4: Verify FPS updates smoothly**

The FPS value should update smoothly (not jumping wildly frame to frame) due to the 100-frame rolling average.

- [ ] **Step 5: Verify host restart resilience**

Ctrl+C → restart host → FPS should reappear after a few frames (FrameStats needs ~10 frames before `available()` returns true).

- [ ] **Step 6: Commit any fixes**

```bash
git add -A
git commit -m "fix: address issues found during Phase 8 integration test"
```

---

## Phase 8 Complete — Summary

At this point you have:

1. **Real-time FPS** displayed in the overlay (no admin privileges, no ETW)
2. **Frame timing** computed from Present hook timestamps via `QueryPerformanceCounter`
3. **Ring buffer** of last 1000 frame times for statistics
4. **Percentile computation** — 1% and 0.1% lows (recalculated every 100 frames)
5. **Widget text override** — DLL overrides host's "FPS: N/A" with computed values
6. **Self-contained module** — `FrameStats` has no dependencies on shared memory or renderer

**Next:** Phase 9a adds the widget file format and CSS styling system.
