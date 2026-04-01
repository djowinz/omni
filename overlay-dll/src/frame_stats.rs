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
/// How often to refresh the displayed values (in frames).
/// At 60fps this is ~2 updates/sec, at 144fps ~5 updates/sec.
const DISPLAY_UPDATE_INTERVAL: usize = 30;

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
    /// Display snapshot — refreshed every DISPLAY_UPDATE_INTERVAL frames.
    /// This is what the overlay reads, preventing jittery text updates.
    display_fps: f32,
    display_frame_time_ms: f32,
    display_avg_ms: f32,
    display_1pct_ms: f32,
    display_01pct_ms: f32,
}

impl FrameStats {
    /// Create a new FrameStats. Call once during renderer init.
    pub fn new() -> Self {
        let mut freq: i64 = 0;
        // SAFETY: QueryPerformanceFrequency always succeeds on Windows XP+.
        // Writes to a stack-local i64.
        unsafe {
            let _ = QueryPerformanceFrequency(&mut freq);
        }

        let mut qpc: i64 = 0;
        // SAFETY: QueryPerformanceCounter always succeeds on Windows XP+.
        // Writes to a stack-local i64.
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
            display_fps: 0.0,
            display_frame_time_ms: 0.0,
            display_avg_ms: 0.0,
            display_1pct_ms: 0.0,
            display_01pct_ms: 0.0,
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
            display_fps: 0.0,
            display_frame_time_ms: 0.0,
            display_avg_ms: 0.0,
            display_1pct_ms: 0.0,
            display_01pct_ms: 0.0,
        }
    }

    /// Record a frame. Call once per Present hook invocation.
    pub fn record(&mut self) {
        let mut qpc: i64 = 0;
        // SAFETY: QueryPerformanceCounter always succeeds on Windows XP+.
        // Writes to a stack-local i64.
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

                // Refresh display snapshot periodically (prevents jittery text)
                if self.total_frames % DISPLAY_UPDATE_INTERVAL as u64 == 0 {
                    self.refresh_display();
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

                if self.total_frames % DISPLAY_UPDATE_INTERVAL as u64 == 0 {
                    self.refresh_display();
                }
            }
        }

        self.last_qpc = qpc;
    }

    /// Refresh the display snapshot with current computed values.
    fn refresh_display(&mut self) {
        self.display_fps = self.compute_fps();
        self.display_frame_time_ms = self.compute_frame_time_ms();
        self.display_avg_ms = self.compute_frame_time_avg_ms();
        self.display_1pct_ms = self.cached_1pct_ms;
        self.display_01pct_ms = self.cached_01pct_ms;
    }

    /// Displayed FPS (updated every DISPLAY_UPDATE_INTERVAL frames).
    pub fn fps(&self) -> f32 {
        self.display_fps
    }

    /// Displayed frame time (updated every DISPLAY_UPDATE_INTERVAL frames).
    pub fn frame_time_ms(&self) -> f32 {
        self.display_frame_time_ms
    }

    /// Displayed average frame time (updated every DISPLAY_UPDATE_INTERVAL frames).
    pub fn frame_time_avg_ms(&self) -> f32 {
        self.display_avg_ms
    }

    /// Internal FPS computation — rolling average over the last FPS_SMOOTHING_FRAMES frames.
    fn compute_fps(&self) -> f32 {
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

    /// Internal: latest frame time in milliseconds.
    fn compute_frame_time_ms(&self) -> f32 {
        if self.count == 0 {
            return 0.0;
        }
        let idx = (self.head + RING_BUFFER_SIZE - 1) % RING_BUFFER_SIZE;
        self.ring[idx]
    }

    /// Internal: average frame time over the entire ring buffer.
    fn compute_frame_time_avg_ms(&self) -> f32 {
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
        self.display_1pct_ms
    }

    /// 0.1% low frame time (99.9th percentile — the worst 0.1% of frames).
    pub fn frame_time_01pct_ms(&self) -> f32 {
        self.display_01pct_ms
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
        // Use freq = 1_000_000 so 1000 ticks = 1ms (avoids truncation errors)
        let freq = 1_000_000.0;
        let mut stats = FrameStats::new_with_freq(freq);
        let mut qpc = 1_000_000i64; // start at some non-zero value
        stats.last_qpc = qpc;

        for &ft in frame_times_ms {
            qpc += (ft as f64 * (freq / 1000.0)) as i64; // convert ms to ticks
            stats.record_with_qpc(qpc);
        }

        // Force display snapshot refresh so tests can read displayed values
        stats.refresh_display();

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
            "1% low should be >=40ms, got {}", stats.frame_time_1pct_ms());
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
