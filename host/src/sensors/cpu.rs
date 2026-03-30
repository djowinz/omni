use omni_shared::CpuData;
use sysinfo::System;
use tracing::info;

pub struct CpuPoller;

impl CpuPoller {
    pub fn new(system: &System) -> Self {
        let core_count = system.cpus().len();
        info!(core_count, "sysinfo: CPU sensor initialized");
        Self
    }

    /// Read CPU data from a shared sysinfo::System instance.
    /// The System must have been refreshed with refresh_cpu_all() before calling.
    pub fn poll(&self, system: &System) -> CpuData {
        let cpus = system.cpus();
        let core_count = cpus.len().min(32) as u32;

        let mut per_core_usage = [-1.0f32; 32];
        let mut per_core_freq_mhz = [0u32; 32];

        for (i, cpu) in cpus.iter().enumerate().take(32) {
            per_core_usage[i] = cpu.cpu_usage();
            per_core_freq_mhz[i] = cpu.frequency() as u32;
        }

        let total_usage = system.global_cpu_usage();

        CpuData {
            total_usage_percent: total_usage,
            per_core_usage,
            core_count,
            per_core_freq_mhz,
            package_temp_c: f32::NAN, // set by CpuTempPoller separately
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_poller_returns_valid_core_count() {
        let mut system = System::new();
        system.refresh_cpu_all();
        std::thread::sleep(std::time::Duration::from_millis(200));
        system.refresh_cpu_all();
        let poller = CpuPoller::new(&system);
        let data = poller.poll(&system);
        assert!(data.core_count > 0, "Should detect at least one CPU core");
        assert!(data.core_count <= 32, "Core count should be capped at 32");
    }

    #[test]
    fn cpu_poller_unused_cores_are_negative() {
        let mut system = System::new();
        system.refresh_cpu_all();
        std::thread::sleep(std::time::Duration::from_millis(200));
        system.refresh_cpu_all();
        let poller = CpuPoller::new(&system);
        let data = poller.poll(&system);
        for i in data.core_count as usize..32 {
            assert_eq!(data.per_core_usage[i], -1.0);
        }
    }

    #[test]
    fn cpu_poller_temp_is_nan() {
        let mut system = System::new();
        system.refresh_cpu_all();
        let poller = CpuPoller::new(&system);
        let data = poller.poll(&system);
        assert!(data.package_temp_c.is_nan(), "Temp should be NaN (set by CpuTempPoller)");
    }
}
