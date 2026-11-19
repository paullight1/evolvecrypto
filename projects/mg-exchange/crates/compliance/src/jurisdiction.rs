use std::collections::HashMap;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Top-level jurisdiction identifier.
///
/// Covers the major crypto regulatory environments.  New jurisdictions can be
/// added without breaking existing rules by extending this enum and inserting
/// a new entry in `JurisdictionEngine::new()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Jurisdiction {
    EU,
    US,
    UK,
    SG,
    AE,
    UA,
    JP,
    KR,
    AU,
    HK,
    TR,
    BR,
    Other,
}

impl Jurisdiction {
    /// Parse a jurisdiction from an ISO-like code string (case-insensitive).
    pub fn from_code(code: &str) -> Option<Self> {
        match code.to_uppercase().as_str() {
            "EU" => Some(Self::EU),
            "US" => Some(Self::US),
            "UK" => Some(Self::UK),
            "SG" => Some(Self::SG),
            "AE" => Some(Self::AE),
            "UA" => Some(Self::UA),
            "JP" => Some(Self::JP),
            "KR" => Some(Self::KR),
            "AU" => Some(Self::AU),
            "HK" => Some(Self::HK),
            "TR" => Some(Self::TR),
            "BR" => Some(Self::BR),
            "OTHER" => Some(Self::Other),
            _ => None,
        }
    }

    /// Canonical code string for this jurisdiction.
    pub fn code(&self) -> &'static str {
        match self {
            Self::EU => "EU",
            Self::US => "US",
            Self::UK => "UK",
            Self::SG => "SG",
            Self::AE => "AE",
            Self::UA => "UA",
            Self::JP => "JP",
            Self::KR => "KR",
            Self::AU => "AU",
            Self::HK => "HK",
            Self::TR => "TR",
            Self::BR => "BR",
            Self::Other => "OTHER",
        }
    }
}

/// Product and risk rules that apply to users in a given jurisdiction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JurisdictionRules {
    pub jurisdiction: Jurisdiction,
    /// Maximum allowed leverage (e.g. 2 for FCA retail, 125 for unrestricted).
    pub max_leverage: u32,
    pub futures_allowed: bool,
    pub options_allowed: bool,
    pub margin_trading_allowed: bool,
    /// Minimum KYC level required: 0 = none, 1 = basic, 2 = full.
    pub kyc_required_level: u8,
    pub max_daily_withdrawal_usd: Decimal,
    /// FATF Travel Rule threshold in USD — transfers at or above this amount
    /// require originator/beneficiary information to be transmitted.
    pub travel_rule_threshold_usd: Decimal,
    pub mandatory_risk_warnings: bool,
    pub fiat_allowed: bool,
    /// Tickers that may not be traded in this jurisdiction (regulatory ban).
    pub restricted_coins: Vec<String>,
}

/// Jurisdiction rule engine — central source of truth for per-country restrictions.
///
/// Pre-populated with rules for the most relevant crypto jurisdictions.
/// Rules can be updated at runtime via `update_rules()` without restarting.
pub struct JurisdictionEngine {
    rules: HashMap<Jurisdiction, JurisdictionRules>,
}

impl JurisdictionEngine {
    /// Create the engine with built-in default rules for all known jurisdictions.
    pub fn new() -> Self {
        let mut rules = HashMap::new();

        rules.insert(
            Jurisdiction::EU,
            JurisdictionRules {
                jurisdiction: Jurisdiction::EU,
                max_leverage: 125,
                futures_allowed: true,
                options_allowed: true,
                margin_trading_allowed: true,
                kyc_required_level: 2,
                max_daily_withdrawal_usd: Decimal::from(100_000),
                travel_rule_threshold_usd: Decimal::from(1000),
                mandatory_risk_warnings: true,
                fiat_allowed: true,
                restricted_coins: vec![],
            },
        );

        rules.insert(
            Jurisdiction::US,
            JurisdictionRules {
                jurisdiction: Jurisdiction::US,
                // CFTC/SEC restrictions: retail crypto derivatives largely prohibited
                max_leverage: 5,
                futures_allowed: false,
                options_allowed: false,
                margin_trading_allowed: false,
                kyc_required_level: 2,
                max_daily_withdrawal_usd: Decimal::from(50_000),
                travel_rule_threshold_usd: Decimal::from(3000),
                mandatory_risk_warnings: true,
                fiat_allowed: true,
                restricted_coins: vec!["XRP".into()],
            },
        );

        rules.insert(
            Jurisdiction::UK,
            JurisdictionRules {
                jurisdiction: Jurisdiction::UK,
                // FCA banned crypto derivatives for retail (PS20/10)
                max_leverage: 2,
                futures_allowed: false,
                options_allowed: false,
                margin_trading_allowed: false,
                kyc_required_level: 2,
                max_daily_withdrawal_usd: Decimal::from(100_000),
                travel_rule_threshold_usd: Decimal::from(1000),
                mandatory_risk_warnings: true,
                fiat_allowed: true,
                restricted_coins: vec![],
            },
        );

        rules.insert(
            Jurisdiction::UA,
            JurisdictionRules {
                jurisdiction: Jurisdiction::UA,
                max_leverage: 100,
                futures_allowed: true,
                options_allowed: true,
                margin_trading_allowed: true,
                // Diia digital ID enables streamlined KYC in Ukraine
                kyc_required_level: 1,
                max_daily_withdrawal_usd: Decimal::from(100_000),
                travel_rule_threshold_usd: Decimal::from(1000),
                mandatory_risk_warnings: true,
                fiat_allowed: true,
                restricted_coins: vec![],
            },
        );

        rules.insert(
            Jurisdiction::SG,
            JurisdictionRules {
                jurisdiction: Jurisdiction::SG,
                max_leverage: 50,
                futures_allowed: true,
                options_allowed: true,
                margin_trading_allowed: true,
                kyc_required_level: 2,
                max_daily_withdrawal_usd: Decimal::from(200_000),
                travel_rule_threshold_usd: Decimal::from(1500),
                mandatory_risk_warnings: true,
                fiat_allowed: true,
                restricted_coins: vec![],
            },
        );

        rules.insert(
            Jurisdiction::AE,
            JurisdictionRules {
                jurisdiction: Jurisdiction::AE,
                max_leverage: 100,
                futures_allowed: true,
                options_allowed: true,
                margin_trading_allowed: true,
                kyc_required_level: 2,
                max_daily_withdrawal_usd: Decimal::from(500_000),
                travel_rule_threshold_usd: Decimal::from(1000),
                mandatory_risk_warnings: true,
                fiat_allowed: true,
                restricted_coins: vec![],
            },
        );

        rules.insert(
            Jurisdiction::JP,
            JurisdictionRules {
                jurisdiction: Jurisdiction::JP,
                // FSA sets 2x leverage cap for crypto
                max_leverage: 2,
                futures_allowed: true,
                options_allowed: false,
                margin_trading_allowed: true,
                kyc_required_level: 2,
                max_daily_withdrawal_usd: Decimal::from(100_000),
                // Japan: travel rule applies to all transfers
                travel_rule_threshold_usd: Decimal::ZERO,
                mandatory_risk_warnings: true,
                fiat_allowed: true,
                restricted_coins: vec![],
            },
        );

        rules.insert(
            Jurisdiction::Other,
            JurisdictionRules {
                jurisdiction: Jurisdiction::Other,
                max_leverage: 20,
                futures_allowed: true,
                options_allowed: false,
                margin_trading_allowed: true,
                kyc_required_level: 1,
                max_daily_withdrawal_usd: Decimal::from(50_000),
                travel_rule_threshold_usd: Decimal::from(1000),
                mandatory_risk_warnings: true,
                fiat_allowed: false,
                restricted_coins: vec![],
            },
        );

        Self { rules }
    }

    /// Return the rules for a jurisdiction, or `None` if the jurisdiction is unknown.
    pub fn get_rules(&self, jurisdiction: Jurisdiction) -> Option<&JurisdictionRules> {
        self.rules.get(&jurisdiction)
    }

    /// Return rules for all registered jurisdictions.
    pub fn all_rules(&self) -> Vec<&JurisdictionRules> {
        self.rules.values().collect()
    }

    /// Return `true` if futures trading is permitted in the given jurisdiction.
    pub fn can_trade_futures(&self, jurisdiction: Jurisdiction) -> bool {
        self.rules
            .get(&jurisdiction)
            .map_or(false, |r| r.futures_allowed)
    }

    /// Return the maximum leverage allowed in the given jurisdiction.
    ///
    /// Falls back to 20x for unknown jurisdictions.
    pub fn max_leverage(&self, jurisdiction: Jurisdiction) -> u32 {
        self.rules
            .get(&jurisdiction)
            .map_or(20, |r| r.max_leverage)
    }

    /// Replace or insert rules for a jurisdiction (used by the admin API).
    pub fn update_rules(&mut self, jurisdiction: Jurisdiction, rules: JurisdictionRules) {
        self.rules.insert(jurisdiction, rules);
    }
}

impl Default for JurisdictionEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_us_futures_not_allowed() {
        let engine = JurisdictionEngine::new();
        assert!(!engine.can_trade_futures(Jurisdiction::US));
    }

    #[test]
    fn test_eu_futures_allowed() {
        let engine = JurisdictionEngine::new();
        assert!(engine.can_trade_futures(Jurisdiction::EU));
    }

    #[test]
    fn test_uk_leverage_restricted() {
        let engine = JurisdictionEngine::new();
        assert_eq!(engine.max_leverage(Jurisdiction::UK), 2);
    }

    #[test]
    fn test_ae_high_withdrawal_limit() {
        let engine = JurisdictionEngine::new();
        let rules = engine.get_rules(Jurisdiction::AE).unwrap();
        assert_eq!(rules.max_daily_withdrawal_usd, Decimal::from(500_000));
    }

    #[test]
    fn test_jp_zero_travel_rule_threshold() {
        let engine = JurisdictionEngine::new();
        let rules = engine.get_rules(Jurisdiction::JP).unwrap();
        assert_eq!(rules.travel_rule_threshold_usd, Decimal::ZERO);
    }

    #[test]
    fn test_unknown_jurisdiction_fallback_leverage() {
        let engine = JurisdictionEngine::new();
        // KR is defined in the enum but not in the default map
        assert_eq!(engine.max_leverage(Jurisdiction::KR), 20);
    }

    #[test]
    fn test_from_code_roundtrip() {
        assert_eq!(Jurisdiction::from_code("EU"), Some(Jurisdiction::EU));
        assert_eq!(Jurisdiction::from_code("us"), Some(Jurisdiction::US));
        assert_eq!(Jurisdiction::from_code("XX"), None);
    }

    #[test]
    fn test_update_rules() {
        let mut engine = JurisdictionEngine::new();
        let custom = JurisdictionRules {
            jurisdiction: Jurisdiction::KR,
            max_leverage: 10,
            futures_allowed: true,
            options_allowed: false,
            margin_trading_allowed: true,
            kyc_required_level: 2,
            max_daily_withdrawal_usd: Decimal::from(80_000),
            travel_rule_threshold_usd: Decimal::from(1000),
            mandatory_risk_warnings: true,
            fiat_allowed: true,
            restricted_coins: vec![],
        };
        engine.update_rules(Jurisdiction::KR, custom);
        assert_eq!(engine.max_leverage(Jurisdiction::KR), 10);
    }
}
