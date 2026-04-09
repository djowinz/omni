pub mod cpu;
pub mod cpu_temp;
pub mod gpu;
pub mod hwinfo;
pub mod ram;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use omni_shared::SensorSnapshot;
use sysinfo::System;
use tracing::info;

use cpu::CpuPoller;
use cpu_temp::CpuTempPoller;
use gpu::GpuPoller;
use hwinfo::HwInfoReader;
use ram::RamPoller;

/// Default poll interval for sensors not explicitly configured.
const DEFAULT_POLL_MS: u64 = 1000;
/// Minimum base tick to avoid busy-spinning.
const MIN_BASE_TICK_MS: u64 = 50;

/// Sensor groups that poll together.
struct SensorGroup {
    interval_ms: u64,
    last_poll: Instant,
}

/// Runs sensor polling on a background thread, sending snapshots via channel.
pub struct SensorPoller {
    handle: Option<thread::JoinHandle<()>>,
    running: Arc<AtomicBool>,
}

impl SensorPoller {
    /// Spawn the sensor polling thread. Returns the poller handle and a receiver
    /// for sensor snapshots. `poll_config` maps sensor names (e.g. "cpu.usage",
    /// "gpu.temp") to poll intervals in milliseconds. Each sensor group uses the
    /// minimum interval of any sensor in that group.
    pub fn start(
        poll_config: HashMap<String, u64>,
        running: Arc<AtomicBool>,
    ) -> (Self, mpsc::Receiver<SensorSnapshot>, mpsc::Receiver<(hwinfo::HwInfoState, bool)>) {
        let (tx, rx) = mpsc::channel();
        let (hwinfo_tx, hwinfo_rx) = mpsc::channel();
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
            let mut hwinfo_reader = HwInfoReader::new();

            if gpu.is_some() {
                info!("sensor suite: CPU + GPU (NVAPI) + RAM + CPU temp (WMI)");
            } else {
                info!("sensor suite: CPU + RAM + CPU temp (WMI) — no NVIDIA GPU detected");
            }

            // Determine per-group intervals
            let cpu_interval = *["cpu.usage", "cpu.temp"]
                .iter()
                .filter_map(|k| poll_config.get(*k))
                .min()
                .unwrap_or(&DEFAULT_POLL_MS);

            let gpu_interval = *[
                "gpu.usage",
                "gpu.temp",
                "gpu.clock",
                "gpu.mem-clock",
                "gpu.vram",
                "gpu.power",
                "gpu.fan",
            ]
            .iter()
            .filter_map(|k| poll_config.get(*k))
            .min()
            .unwrap_or(&DEFAULT_POLL_MS);

            let ram_interval = *["ram.usage"]
                .iter()
                .filter_map(|k| poll_config.get(*k))
                .min()
                .unwrap_or(&DEFAULT_POLL_MS);

            let base_tick =
                gcd(gcd(cpu_interval, gpu_interval), ram_interval).max(MIN_BASE_TICK_MS);

            info!(
                cpu_ms = cpu_interval,
                gpu_ms = gpu_interval,
                ram_ms = ram_interval,
                base_tick_ms = base_tick,
                "Sensor polling configured"
            );

            let now = Instant::now();
            let mut cpu_group = SensorGroup {
                interval_ms: cpu_interval,
                last_poll: now,
            };
            let mut gpu_group = SensorGroup {
                interval_ms: gpu_interval,
                last_poll: now,
            };
            let mut ram_group = SensorGroup {
                interval_ms: ram_interval,
                last_poll: now,
            };

            // sysinfo needs two samples to compute CPU usage
            thread::sleep(Duration::from_millis(500));
            info!("Sensor polling started");

            let mut snapshot = SensorSnapshot::default();

            while running_clone.load(Ordering::Relaxed) {
                let now = Instant::now();
                let mut any_updated = false;

                // CPU group
                if now.duration_since(cpu_group.last_poll).as_millis()
                    >= cpu_group.interval_ms as u128
                {
                    system.refresh_cpu_all();
                    let mut cpu_data = cpu.poll(&system);
                    cpu_data.package_temp_c = cpu_temp.poll();
                    snapshot.cpu = cpu_data;
                    cpu_group.last_poll = now;
                    any_updated = true;
                }

                // GPU group
                if now.duration_since(gpu_group.last_poll).as_millis()
                    >= gpu_group.interval_ms as u128
                {
                    snapshot.gpu = match &gpu {
                        Some(g) => g.poll(),
                        None => omni_shared::GpuData::default(),
                    };
                    gpu_group.last_poll = now;
                    any_updated = true;
                }

                // RAM group
                if now.duration_since(ram_group.last_poll).as_millis()
                    >= ram_group.interval_ms as u128
                {
                    system.refresh_memory();
                    snapshot.ram = ram.poll(&system);
                    ram_group.last_poll = now;
                    any_updated = true;
                }

                // HWiNFO group (polled every tick — lightweight shared memory read).
                // Note: HWiNFO is not included in the base tick GCD calculation.
                // If all sensor groups have high intervals the effective HWiNFO poll
                // rate equals the base tick, which is still fine since the read is
                // a sub-microsecond memcpy.
                {
                    let (hwinfo_state, hwinfo_sensors_changed) = hwinfo_reader.poll();
                    snapshot.hwinfo_connected = hwinfo_state.connected;
                    snapshot.hwinfo_sensor_count = hwinfo_state.sensor_count;
                    if hwinfo_state.connected {
                        any_updated = true;
                    }
                    if hwinfo_state.connected || hwinfo_sensors_changed {
                        let _ = hwinfo_tx.send((hwinfo_state.clone(), hwinfo_sensors_changed));
                    }
                }

                if any_updated {
                    snapshot.timestamp_ms = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64;

                    if tx.send(snapshot).is_err() {
                        break;
                    }
                }

                thread::sleep(Duration::from_millis(base_tick));
            }
        });

        (
            Self {
                handle: Some(handle),
                running,
            },
            rx,
            hwinfo_rx,
        )
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

/// Greatest common divisor.
fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 {
        a
    } else {
        gcd(b, a % b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gcd_computation() {
        assert_eq!(gcd(1000, 250), 250);
        assert_eq!(gcd(100, 1000), 100);
        assert_eq!(gcd(300, 200), 100);
        assert_eq!(gcd(0, 500), 500);
    }
}
