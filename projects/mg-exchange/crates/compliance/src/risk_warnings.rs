use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WarningType {
    FuturesTrading,
    HighLeverage,
    LiquidationRisk,
    VolatileAsset,
    MarginTrading,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskWarning {
    pub warning_type: WarningType,
    pub title: String,
    pub text: String,
}

/// Tracks which risk warnings each user has acknowledged.
pub struct RiskWarningManager {
    warnings: Vec<RiskWarning>,
    acknowledgements: HashMap<Uuid, HashMap<WarningType, DateTime<Utc>>>,
}

impl RiskWarningManager {
    pub fn new() -> Self {
        Self {
            warnings: vec![
                RiskWarning {
                    warning_type: WarningType::FuturesTrading,
                    title: "Futures Trading Risk".into(),
                    text: "Trading cryptocurrency futures involves significant risk of loss. You may lose more than your initial investment. Past performance is not indicative of future results. Only trade with funds you can afford to lose.".into(),
                },
                RiskWarning {
                    warning_type: WarningType::HighLeverage,
                    title: "High Leverage Risk".into(),
                    text: "Using leverage amplifies both potential gains and losses. Higher leverage increases the risk of liquidation. A small price movement against your position can result in the loss of your entire margin.".into(),
                },
                RiskWarning {
                    warning_type: WarningType::LiquidationRisk,
                    title: "Liquidation Risk".into(),
                    text: "When your margin falls below the maintenance requirement, your position will be automatically liquidated at the current market price. You will lose the margin allocated to the position.".into(),
                },
                RiskWarning {
                    warning_type: WarningType::MarginTrading,
                    title: "Margin Trading Risk".into(),
                    text: "Margin trading involves borrowing funds and carries interest charges. If the value of your collateral falls below the required level, your positions may be forcibly closed.".into(),
                },
            ],
            acknowledgements: HashMap::new(),
        }
    }

    /// Get all warnings that a user hasn't acknowledged.
    pub fn pending_warnings(&self, user_id: Uuid) -> Vec<&RiskWarning> {
        let acked = self.acknowledgements.get(&user_id);
        self.warnings
            .iter()
            .filter(|w| acked.map_or(true, |map| !map.contains_key(&w.warning_type)))
            .collect()
    }

    /// Acknowledge a warning.
    pub fn acknowledge(&mut self, user_id: Uuid, warning_type: WarningType) {
        self.acknowledgements
            .entry(user_id)
            .or_default()
            .insert(warning_type, Utc::now());
    }

    /// Check if a user has acknowledged a specific warning.
    pub fn has_acknowledged(&self, user_id: Uuid, warning_type: WarningType) -> bool {
        self.acknowledgements
            .get(&user_id)
            .map_or(false, |map| map.contains_key(&warning_type))
    }

    /// Check if user can trade futures (must acknowledge FuturesTrading + LiquidationRisk).
    pub fn can_trade_futures(&self, user_id: Uuid) -> bool {
        self.has_acknowledged(user_id, WarningType::FuturesTrading)
            && self.has_acknowledged(user_id, WarningType::LiquidationRisk)
    }

    /// All available warnings.
    pub fn all_warnings(&self) -> &[RiskWarning] {
        &self.warnings
    }
}

impl Default for RiskWarningManager {
    fn default() -> Self {
        Self::new()
    }
}
