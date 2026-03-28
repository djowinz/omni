pub mod cpu;

use std::sync::mpsc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use omni_shared::SensorSnapshot;

use cpu::CpuPoller;

/// Runs sensor polling on a background thread, sending snapshots via channel.
pub struct SensorPoller {
    handle: Option<thread::JoinHandle<()>>,
    running: Arc<AtomicBool>,
}

impl SensorPoller {
    /// Spawn the sensor polling thread. Returns the poller handle and a receiver
    /// for sensor snapshots.
    pub fn start(interval: Duration, running: Arc<AtomicBool>) -> (Self, mpsc::Receiver<SensorSnapshot>) {
        let (tx, rx) = mpsc::channel();
        let running_clone = running.clone();

        let handle = thread::spawn(move || {
            let mut cpu = CpuPoller::new();

            // sysinfo needs two samples to compute usage — wait before first real poll
            thread::sleep(Duration::from_millis(500));

            while running_clone.load(Ordering::Relaxed) {
                let cpu_data = cpu.poll();

                let snapshot = SensorSnapshot {
                    timestamp_ms: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64,
                    cpu: cpu_data,
                    ..Default::default()
                };

                if tx.send(snapshot).is_err() {
                    break;
                }

                thread::sleep(interval);
            }
        });

        (Self { handle: Some(handle), running }, rx)
    }

    /// Signal the polling thread to stop and wait for it to finish.
    pub fn stop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for SensorPoller {
    fn drop(&mut self) {
        self.stop();
    }
}
