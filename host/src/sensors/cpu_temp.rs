//! CPU temperature via WMI MSAcpi_ThermalZoneTemperature.
//!
//! This is a built-in Windows WMI class that some systems expose without
//! third-party software. Returns temperature in tenths of Kelvin.
//! If the query fails (common on many systems), returns f32::NAN.

use serde::Deserialize;
use tracing::{info, warn};
use wmi::{COMLibrary, WMIConnection};

#[derive(Deserialize)]
#[serde(rename = "MSAcpi_ThermalZoneTemperature")]
#[serde(rename_all = "PascalCase")]
struct ThermalZone {
    current_temperature: u32, // tenths of Kelvin
}

/// Converts WMI thermal zone value (tenths of Kelvin) to Celsius.
pub fn tenths_kelvin_to_celsius(tenths_k: u32) -> f32 {
    (tenths_k as f32 / 10.0) - 273.15
}

pub struct CpuTempPoller {
    connection: Option<WMIConnection>,
}

impl CpuTempPoller {
    /// Initialize WMI connection to root\WMI namespace.
    /// Returns a poller that always works — if WMI connection fails,
    /// poll() will return NaN.
    pub fn new() -> Self {
        let connection = Self::connect();
        Self { connection }
    }

    fn connect() -> Option<WMIConnection> {
        let com = COMLibrary::without_security().ok()?;
        match WMIConnection::with_namespace_path("root\\WMI", com) {
            Ok(conn) => {
                info!("WMI: connected to root\\WMI for CPU temperature");
                Some(conn)
            }
            Err(e) => {
                warn!(error = %e, "WMI: failed to connect to root\\WMI — CPU temperature unavailable");
                None
            }
        }
    }

    /// Query CPU temperature. Returns Celsius or NaN if unavailable.
    pub fn poll(&self) -> f32 {
        let conn = match &self.connection {
            Some(c) => c,
            None => return f32::NAN,
        };

        match conn.raw_query::<ThermalZone>(
            "SELECT CurrentTemperature FROM MSAcpi_ThermalZoneTemperature",
        ) {
            Ok(results) if !results.is_empty() => {
                tenths_kelvin_to_celsius(results[0].current_temperature)
            }
            Ok(_) => f32::NAN,  // Query succeeded but no results
            Err(_) => f32::NAN, // Query failed
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tenths_kelvin_conversion() {
        // 3000 tenths of Kelvin = 300K = 26.85°C
        let celsius = tenths_kelvin_to_celsius(3000);
        assert!(
            (celsius - 26.85).abs() < 0.01,
            "Expected ~26.85°C, got {celsius}"
        );
    }

    #[test]
    fn boiling_point_conversion() {
        // 3731 tenths of Kelvin = 373.1K = 99.95°C
        let celsius = tenths_kelvin_to_celsius(3731);
        assert!(
            (celsius - 99.95).abs() < 0.01,
            "Expected ~99.95°C, got {celsius}"
        );
    }

    #[test]
    fn cpu_temp_poller_does_not_panic() {
        // Just verify it doesn't crash — result may be NaN on systems without WMI thermal
        let poller = CpuTempPoller::new();
        let temp = poller.poll();
        // temp is either a valid number or NaN — both are fine
        assert!(
            temp.is_nan() || (temp > -50.0 && temp < 150.0),
            "Temperature should be NaN or reasonable, got {temp}"
        );
    }
}
