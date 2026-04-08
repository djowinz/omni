//! RAM sensor via sysinfo.

use omni_shared::RamData;
use sysinfo::System;

pub struct RamPoller;

impl RamPoller {
    pub fn new() -> Self {
        Self
    }

    /// Poll RAM data from a shared sysinfo::System instance.
    /// The System must have been refreshed with refresh_memory() before calling.
    pub fn poll(&self, system: &System) -> RamData {
        let total_bytes = system.total_memory();
        let used_bytes = system.used_memory();

        let total_mb = total_bytes / (1024 * 1024);
        let used_mb = used_bytes / (1024 * 1024);

        let usage_percent = if total_mb > 0 {
            (used_mb as f32 / total_mb as f32) * 100.0
        } else {
            0.0
        };

        RamData {
            usage_percent,
            used_mb,
            total_mb,
            frequency_mhz: 0, // requires LHM/WMI — deferred
            timing_cl: 0,     // requires LHM/WMI — deferred
            temp_c: f32::NAN, // requires LHM/WMI — deferred
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ram_poller_returns_nonzero() {
        let mut system = System::new();
        system.refresh_memory();
        let poller = RamPoller::new();
        let data = poller.poll(&system);

        assert!(data.total_mb > 0, "System should have some RAM");
        assert!(data.used_mb > 0, "Some RAM should be in use");
        assert!(
            data.usage_percent > 0.0 && data.usage_percent <= 100.0,
            "Usage should be 0-100%, got {}",
            data.usage_percent
        );
    }

    #[test]
    fn ram_deferred_fields_are_unavailable() {
        let mut system = System::new();
        system.refresh_memory();
        let poller = RamPoller::new();
        let data = poller.poll(&system);

        assert_eq!(data.frequency_mhz, 0);
        assert_eq!(data.timing_cl, 0);
        assert!(data.temp_c.is_nan());
    }
}
