use omni_shared::CpuData;
use sysinfo::System;
use tracing::info;

pub struct CpuPoller {
    system: System,
}

impl CpuPoller {
    pub fn new() -> Self {
        let mut system = System::new();
        system.refresh_cpu_all();

        let core_count = system.cpus().len();
        info!(core_count, "sysinfo: CPU sensor initialized");

        Self { system }
    }

    /// Refresh CPU data and return a populated CpuData struct.
    pub fn poll(&mut self) -> CpuData {
        self.system.refresh_cpu_all();

        let cpus = self.system.cpus();
        let core_count = cpus.len().min(32) as u32;

        let mut per_core_usage = [-1.0f32; 32];
        let mut per_core_freq_mhz = [0u32; 32];

        for (i, cpu) in cpus.iter().enumerate().take(32) {
            per_core_usage[i] = cpu.cpu_usage();
            per_core_freq_mhz[i] = cpu.frequency() as u32;
        }

        let total_usage = self.system.global_cpu_usage();

        CpuData {
            total_usage_percent: total_usage,
            per_core_usage,
            core_count,
            per_core_freq_mhz,
            package_temp_c: f32::NAN,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_poller_returns_valid_core_count() {
        let mut poller = CpuPoller::new();
        std::thread::sleep(std::time::Duration::from_millis(200));
        let data = poller.poll();
        assert!(data.core_count > 0, "Should detect at least one CPU core");
        assert!(data.core_count <= 32, "Core count should be capped at 32");
    }

    #[test]
    fn cpu_poller_unused_cores_are_negative() {
        let mut poller = CpuPoller::new();
        std::thread::sleep(std::time::Duration::from_millis(200));
        let data = poller.poll();
        for i in data.core_count as usize..32 {
            assert_eq!(data.per_core_usage[i], -1.0);
        }
    }

    #[test]
    fn cpu_poller_temp_is_nan() {
        let mut poller = CpuPoller::new();
        let data = poller.poll();
        assert!(data.package_temp_c.is_nan(), "Temp should be NaN without LHM");
    }
}
