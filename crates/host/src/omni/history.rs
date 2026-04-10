//! Rolling history buffers for sensor values.
//!
//! Only sensors explicitly registered (because a chart references them)
//! get a buffer. The buffer is a fixed-capacity ring — oldest sample is
//! dropped when a new one arrives at capacity.

use std::collections::{HashMap, HashSet, VecDeque};

/// Default buffer capacity — 60 samples gives one minute of history at 1 Hz,
/// or proportionally less at faster poll intervals.
pub const DEFAULT_BUFFER_CAPACITY: usize = 60;

pub struct SensorHistory {
    buffers: HashMap<String, VecDeque<f64>>,
    capacity: usize,
    registered: HashSet<String>,
}

impl SensorHistory {
    pub fn new() -> Self {
        Self {
            buffers: HashMap::new(),
            capacity: DEFAULT_BUFFER_CAPACITY,
            registered: HashSet::new(),
        }
    }

    #[allow(dead_code)] // Used by tests; public API for future non-default buffer sizes
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            buffers: HashMap::new(),
            capacity,
            registered: HashSet::new(),
        }
    }

    /// Register a sensor path so its samples will be buffered.
    /// Registering an already-registered sensor is a no-op.
    pub fn register(&mut self, sensor_path: &str) {
        if self.registered.insert(sensor_path.to_string()) {
            self.buffers
                .entry(sensor_path.to_string())
                .or_insert_with(|| VecDeque::with_capacity(self.capacity));
        }
    }

    #[allow(dead_code)] // Used by tests; public API for future diagnostics
    pub fn is_registered(&self, sensor_path: &str) -> bool {
        self.registered.contains(sensor_path)
    }

    /// Push a sample for a registered sensor. Unregistered sensors are
    /// ignored (no buffer allocated). When capacity is reached, the
    /// oldest sample is dropped.
    pub fn push_sample(&mut self, sensor_path: &str, value: f64) {
        if !self.registered.contains(sensor_path) {
            return;
        }
        let capacity = self.capacity;
        let buf = self
            .buffers
            .entry(sensor_path.to_string())
            .or_insert_with(|| VecDeque::with_capacity(capacity));
        if buf.len() >= capacity {
            buf.pop_front();
        }
        buf.push_back(value);
    }

    pub fn buffer(&self, sensor_path: &str) -> Option<&VecDeque<f64>> {
        self.buffers.get(sensor_path)
    }

    pub fn min(&self, sensor_path: &str) -> Option<f64> {
        self.buffers.get(sensor_path).and_then(|b| {
            b.iter()
                .cloned()
                .fold(None, |acc, v| Some(acc.map(|a: f64| a.min(v)).unwrap_or(v)))
        })
    }

    pub fn max(&self, sensor_path: &str) -> Option<f64> {
        self.buffers.get(sensor_path).and_then(|b| {
            b.iter()
                .cloned()
                .fold(None, |acc, v| Some(acc.map(|a: f64| a.max(v)).unwrap_or(v)))
        })
    }

    pub fn avg(&self, sensor_path: &str) -> Option<f64> {
        let buf = self.buffers.get(sensor_path)?;
        if buf.is_empty() {
            return None;
        }
        let sum: f64 = buf.iter().sum();
        Some(sum / buf.len() as f64)
    }

    /// Clear registration and buffers for sensors not in `keep`. Used on
    /// overlay reload to drop buffers for sensors no longer referenced.
    pub fn clear_unregistered(&mut self, keep: &HashSet<String>) {
        self.registered.retain(|k| keep.contains(k));
        self.buffers.retain(|k, _| keep.contains(k));
    }

    /// Iterate over registered sensor paths.
    pub fn registered_iter(&self) -> impl Iterator<Item = String> + '_ {
        self.registered.iter().cloned()
    }
}

impl Default for SensorHistory {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_unregistered_sensor_is_ignored() {
        let mut h = SensorHistory::new();
        h.push_sample("cpu.usage", 50.0);
        assert!(h.buffer("cpu.usage").is_none());
    }

    #[test]
    fn register_and_push() {
        let mut h = SensorHistory::new();
        h.register("cpu.usage");
        h.push_sample("cpu.usage", 50.0);
        h.push_sample("cpu.usage", 60.0);
        let buf = h.buffer("cpu.usage").unwrap();
        assert_eq!(buf.len(), 2);
        assert_eq!(buf[0], 50.0);
        assert_eq!(buf[1], 60.0);
    }

    #[test]
    fn capacity_enforced() {
        let mut h = SensorHistory::with_capacity(3);
        h.register("cpu.usage");
        h.push_sample("cpu.usage", 1.0);
        h.push_sample("cpu.usage", 2.0);
        h.push_sample("cpu.usage", 3.0);
        h.push_sample("cpu.usage", 4.0);
        let buf = h.buffer("cpu.usage").unwrap();
        assert_eq!(buf.len(), 3);
        assert_eq!(buf[0], 2.0);
        assert_eq!(buf[1], 3.0);
        assert_eq!(buf[2], 4.0);
    }

    #[test]
    fn min_max_avg() {
        let mut h = SensorHistory::new();
        h.register("cpu.usage");
        for v in [10.0, 20.0, 30.0, 40.0, 50.0] {
            h.push_sample("cpu.usage", v);
        }
        assert_eq!(h.min("cpu.usage"), Some(10.0));
        assert_eq!(h.max("cpu.usage"), Some(50.0));
        assert_eq!(h.avg("cpu.usage"), Some(30.0));
    }

    #[test]
    fn empty_buffer_queries_return_none() {
        let mut h = SensorHistory::new();
        h.register("cpu.usage");
        assert_eq!(h.min("cpu.usage"), None);
        assert_eq!(h.max("cpu.usage"), None);
        assert_eq!(h.avg("cpu.usage"), None);
    }

    #[test]
    fn clear_unregistered_drops_buffers() {
        let mut h = SensorHistory::new();
        h.register("cpu.usage");
        h.register("gpu.temp");
        h.push_sample("cpu.usage", 50.0);
        h.push_sample("gpu.temp", 70.0);

        let keep: HashSet<String> = ["cpu.usage".to_string()].into_iter().collect();
        h.clear_unregistered(&keep);

        assert!(h.buffer("cpu.usage").is_some());
        assert!(h.buffer("gpu.temp").is_none());
        assert!(h.is_registered("cpu.usage"));
        assert!(!h.is_registered("gpu.temp"));
    }

    #[test]
    fn double_registration_is_noop() {
        let mut h = SensorHistory::new();
        h.register("cpu.usage");
        h.push_sample("cpu.usage", 50.0);
        h.register("cpu.usage"); // Should not clear buffer
        assert_eq!(h.buffer("cpu.usage").unwrap().len(), 1);
    }
}
