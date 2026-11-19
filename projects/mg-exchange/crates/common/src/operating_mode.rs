use std::sync::atomic::{AtomicU8, Ordering};

/// Exchange operating mode — supports graceful degradation during incidents.
///
/// Modes ordered by restrictiveness:
/// - `Normal` — full operation
/// - `ReadOnly` — no new orders, cancels allowed, market data flows
/// - `CancelOnly` — only cancel orders, no new placements
/// - `MarketDataOnly` — market data visible, all order ops rejected
/// - `Halted` — everything rejected (emergency)
///
/// The principle: never lose customer ability to *reduce risk* (cancel, close)
/// even when you stop accepting new risk — hence `CancelOnly` exists between
/// `ReadOnly` and full `Halted`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[repr(u8)]
pub enum OperatingMode {
    Normal = 0,
    ReadOnly = 1,
    CancelOnly = 2,
    MarketDataOnly = 3,
    Halted = 4,
}

impl OperatingMode {
    /// Can new orders be placed in this mode?
    pub fn allows_new_orders(self) -> bool {
        matches!(self, Self::Normal)
    }

    /// Can existing orders be cancelled in this mode?
    pub fn allows_cancels(self) -> bool {
        matches!(self, Self::Normal | Self::ReadOnly | Self::CancelOnly)
    }

    /// Is market data flowing in this mode?
    pub fn allows_market_data(self) -> bool {
        !matches!(self, Self::Halted)
    }

    fn from_u8(v: u8) -> Self {
        match v {
            0 => Self::Normal,
            1 => Self::ReadOnly,
            2 => Self::CancelOnly,
            3 => Self::MarketDataOnly,
            4 => Self::Halted,
            _ => Self::Halted, // unknown → safest default
        }
    }
}

impl std::fmt::Display for OperatingMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Normal => write!(f, "Normal"),
            Self::ReadOnly => write!(f, "ReadOnly"),
            Self::CancelOnly => write!(f, "CancelOnly"),
            Self::MarketDataOnly => write!(f, "MarketDataOnly"),
            Self::Halted => write!(f, "Halted"),
        }
    }
}

impl std::str::FromStr for OperatingMode {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Normal" | "normal" => Ok(Self::Normal),
            "ReadOnly" | "read_only" | "readonly" => Ok(Self::ReadOnly),
            "CancelOnly" | "cancel_only" | "cancelonly" => Ok(Self::CancelOnly),
            "MarketDataOnly" | "market_data_only" | "marketdataonly" => Ok(Self::MarketDataOnly),
            "Halted" | "halted" => Ok(Self::Halted),
            other => Err(format!("unknown operating mode: '{other}'")),
        }
    }
}

/// Atomic wrapper around `OperatingMode` for lock-free reads on the hot path.
pub struct AtomicOperatingMode {
    inner: AtomicU8,
}

impl AtomicOperatingMode {
    pub fn new(mode: OperatingMode) -> Self {
        Self {
            inner: AtomicU8::new(mode as u8),
        }
    }

    pub fn load(&self) -> OperatingMode {
        OperatingMode::from_u8(self.inner.load(Ordering::Relaxed))
    }

    pub fn store(&self, mode: OperatingMode) {
        self.inner.store(mode as u8, Ordering::Relaxed);
    }
}

impl Default for AtomicOperatingMode {
    fn default() -> Self {
        Self::new(OperatingMode::Normal)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_allows_everything() {
        assert!(OperatingMode::Normal.allows_new_orders());
        assert!(OperatingMode::Normal.allows_cancels());
        assert!(OperatingMode::Normal.allows_market_data());
    }

    #[test]
    fn cancel_only_rejects_new_orders_allows_cancel() {
        assert!(!OperatingMode::CancelOnly.allows_new_orders());
        assert!(OperatingMode::CancelOnly.allows_cancels());
        assert!(OperatingMode::CancelOnly.allows_market_data());
    }

    #[test]
    fn halted_rejects_everything() {
        assert!(!OperatingMode::Halted.allows_new_orders());
        assert!(!OperatingMode::Halted.allows_cancels());
        assert!(!OperatingMode::Halted.allows_market_data());
    }

    #[test]
    fn atomic_round_trip() {
        let a = AtomicOperatingMode::new(OperatingMode::CancelOnly);
        assert_eq!(a.load(), OperatingMode::CancelOnly);
        a.store(OperatingMode::Normal);
        assert_eq!(a.load(), OperatingMode::Normal);
    }

    #[test]
    fn parse_mode_from_string() {
        assert_eq!("Normal".parse::<OperatingMode>().unwrap(), OperatingMode::Normal);
        assert_eq!("cancel_only".parse::<OperatingMode>().unwrap(), OperatingMode::CancelOnly);
        assert_eq!("Halted".parse::<OperatingMode>().unwrap(), OperatingMode::Halted);
        assert!("invalid".parse::<OperatingMode>().is_err());
    }
}
