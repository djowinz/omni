pub mod cpu;
pub mod cpu_temp;
pub mod gpu;
pub mod ram;

use std::sync::mpsc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use omni_shared::SensorSnapshot;
use sysinfo::System;
use tracing::info;

use cpu::CpuPoller;
use cpu_temp::CpuTempPoller;
use gpu::GpuPoller;
use ram::RamPoller;

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
            // Shared sysinfo instance for CPU and RAM
            let mut system = System::new();
            system.refresh_cpu_all();
            system.refresh_memory();

            let cpu = CpuPoller::new(&system);
            let cpu_temp = CpuTempPoller::new();
            let gpu = GpuPoller::new();
            let ram = RamPoller::new();

            if gpu.is_some() {
                info!("sensor suite: CPU + GPU (NVAPI) + RAM + CPU temp (WMI)");
            } else {
                info!("sensor suite: CPU + RAM + CPU temp (WMI) — no NVIDIA GPU detected");
            }

            // sysinfo needs two samples to compute CPU usage
            thread::sleep(Duration::from_millis(500));

            info!("Sensor polling started");

            while running_clone.load(Ordering::Relaxed) {
                system.refresh_cpu_all();
                system.refresh_memory();

                let mut cpu_data = cpu.poll(&system);
                cpu_data.package_temp_c = cpu_temp.poll();

                let gpu_data = match &gpu {
                    Some(g) => g.poll(),
                    None => omni_shared::GpuData::default(),
                };

                let ram_data = ram.poll(&system);

                let snapshot = SensorSnapshot {
                    timestamp_ms: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64,
                    cpu: cpu_data,
                    gpu: gpu_data,
                    ram: ram_data,
                    ..Default::default() // frame data — Phase 8
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
