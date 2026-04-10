//! Unit-aware formatting and nice-tick algorithm for chart values.
//!
//! Units drive two things:
//! - How raw sensor values are formatted for display (e.g., `1500 bytes/s` → `1.5 KB/s`)
//! - How "nice" round-number bounds and tick values are selected for chart Y-axes

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    BytesPerSec,
    BitsPerSec,
    Bytes,
    Hz,
    Celsius,
    Percent,
    Count,
    Ms,
    Seconds,
    Rpm,
    Volts,
    Watts,
    Pascals,
    None,
}

impl Unit {
    /// Parse a unit identifier from a string. Used when the interpolation
    /// parser encounters a bare identifier in a Unit-typed argument position.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "bytes/s" => Some(Unit::BytesPerSec),
            "bits/s" => Some(Unit::BitsPerSec),
            "bytes" => Some(Unit::Bytes),
            "Hz" => Some(Unit::Hz),
            "°C" => Some(Unit::Celsius),
            "%" => Some(Unit::Percent),
            "count" => Some(Unit::Count),
            "ms" => Some(Unit::Ms),
            "s" => Some(Unit::Seconds),
            "RPM" => Some(Unit::Rpm),
            "V" => Some(Unit::Volts),
            "W" => Some(Unit::Watts),
            "Pa" => Some(Unit::Pascals),
            "none" => Some(Unit::None),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_all_units() {
        assert_eq!(Unit::parse("bytes/s"), Some(Unit::BytesPerSec));
        assert_eq!(Unit::parse("bits/s"), Some(Unit::BitsPerSec));
        assert_eq!(Unit::parse("bytes"), Some(Unit::Bytes));
        assert_eq!(Unit::parse("Hz"), Some(Unit::Hz));
        assert_eq!(Unit::parse("°C"), Some(Unit::Celsius));
        assert_eq!(Unit::parse("%"), Some(Unit::Percent));
        assert_eq!(Unit::parse("count"), Some(Unit::Count));
        assert_eq!(Unit::parse("ms"), Some(Unit::Ms));
        assert_eq!(Unit::parse("s"), Some(Unit::Seconds));
        assert_eq!(Unit::parse("RPM"), Some(Unit::Rpm));
        assert_eq!(Unit::parse("V"), Some(Unit::Volts));
        assert_eq!(Unit::parse("W"), Some(Unit::Watts));
        assert_eq!(Unit::parse("Pa"), Some(Unit::Pascals));
        assert_eq!(Unit::parse("none"), Some(Unit::None));
    }

    #[test]
    fn parse_unknown_returns_none() {
        assert_eq!(Unit::parse("fubar"), None);
        assert_eq!(Unit::parse(""), None);
    }
}
