use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A summary tax report for a single user over a calendar year.
///
/// Covers realized gains/losses, total trading volume, fees paid, and funding
/// payments received/paid — the data a user needs to file a crypto tax return.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaxReport {
    pub user_id: Uuid,
    pub year: u32,
    pub total_trades: u64,
    pub total_buy_volume: Decimal,
    pub total_sell_volume: Decimal,
    pub realized_gains: Decimal,
    pub realized_losses: Decimal,
    pub net_pnl: Decimal,
    pub fees_paid: Decimal,
    pub funding_received: Decimal,
    pub funding_paid: Decimal,
    pub generated_at: DateTime<Utc>,
}

/// Generates tax reports from order history and funding data.
pub struct TaxReportGenerator;

impl TaxReportGenerator {
    /// Generate a tax report from order history and funding data.
    ///
    /// Each trade tuple is `(qty, price, fee)`:
    /// - Positive `qty` = buy.
    /// - Negative `qty` = sell.
    ///
    /// `funding_payments` are signed: positive = received, negative = paid.
    ///
    /// Note: PnL calculation uses a simplified model (total sell minus total buy).
    /// A production-grade implementation would use FIFO/LIFO cost-basis accounting.
    pub fn generate(
        user_id: Uuid,
        year: u32,
        trades: &[(Decimal, Decimal, Decimal)], // (qty, price, fee)
        funding_payments: &[Decimal],
    ) -> TaxReport {
        let mut total_buy_vol = Decimal::ZERO;
        let mut total_sell_vol = Decimal::ZERO;
        let mut fees = Decimal::ZERO;

        for (qty, price, fee) in trades {
            let notional = *qty * *price;
            if *qty > Decimal::ZERO {
                total_buy_vol += notional;
            } else {
                total_sell_vol += notional.abs();
            }
            fees += *fee;
        }

        // Simplified PnL: net proceeds from sells minus cost of buys.
        // Real implementation requires FIFO/LIFO cost-basis tracking per asset.
        let net = total_sell_vol - total_buy_vol;
        let (gains, losses) = if net > Decimal::ZERO {
            (net, Decimal::ZERO)
        } else {
            (Decimal::ZERO, net.abs())
        };

        let funding_received: Decimal = funding_payments
            .iter()
            .filter(|&&f| f > Decimal::ZERO)
            .sum();
        let funding_paid: Decimal = funding_payments
            .iter()
            .filter(|&&f| f < Decimal::ZERO)
            .map(|f| f.abs())
            .sum();

        TaxReport {
            user_id,
            year,
            total_trades: trades.len() as u64,
            total_buy_volume: total_buy_vol,
            total_sell_volume: total_sell_vol,
            realized_gains: gains,
            realized_losses: losses,
            net_pnl: gains - losses,
            fees_paid: fees,
            funding_received,
            funding_paid,
            generated_at: Utc::now(),
        }
    }

    /// Render a tax report as CSV for download.
    pub fn to_csv(report: &TaxReport) -> String {
        format!(
            "User ID,Year,Total Trades,Buy Volume,Sell Volume,Realized Gains,Realized Losses,Net PnL,Fees Paid,Funding Received,Funding Paid\n\
             {},{},{},{},{},{},{},{},{},{},{}\n",
            report.user_id,
            report.year,
            report.total_trades,
            report.total_buy_volume,
            report.total_sell_volume,
            report.realized_gains,
            report.realized_losses,
            report.net_pnl,
            report.fees_paid,
            report.funding_received,
            report.funding_paid,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_generate_empty() {
        let report = TaxReportGenerator::generate(Uuid::nil(), 2026, &[], &[]);
        assert_eq!(report.total_trades, 0);
        assert_eq!(report.net_pnl, Decimal::ZERO);
    }

    #[test]
    fn test_generate_basic_gain() {
        // Buy 1 BTC at $60k, sell 1 BTC at $70k
        let trades = vec![
            (dec!(1), dec!(60_000), dec!(30)),   // buy
            (dec!(-1), dec!(70_000), dec!(35)),  // sell
        ];
        let report = TaxReportGenerator::generate(Uuid::nil(), 2026, &trades, &[]);
        assert_eq!(report.total_trades, 2);
        assert_eq!(report.total_buy_volume, dec!(60_000));
        assert_eq!(report.total_sell_volume, dec!(70_000));
        assert_eq!(report.realized_gains, dec!(10_000));
        assert_eq!(report.realized_losses, Decimal::ZERO);
        assert_eq!(report.fees_paid, dec!(65));
    }

    #[test]
    fn test_generate_basic_loss() {
        // Buy 1 ETH at $3000, sell at $2500
        let trades = vec![
            (dec!(1), dec!(3_000), dec!(3)),
            (dec!(-1), dec!(2_500), dec!(2.5)),
        ];
        let report = TaxReportGenerator::generate(Uuid::nil(), 2026, &trades, &[]);
        assert_eq!(report.realized_gains, Decimal::ZERO);
        assert_eq!(report.realized_losses, dec!(500));
        assert_eq!(report.net_pnl, dec!(-500));
    }

    #[test]
    fn test_funding_split() {
        let funding = vec![dec!(10), dec!(-5), dec!(20), dec!(-3)];
        let report = TaxReportGenerator::generate(Uuid::nil(), 2026, &[], &funding);
        assert_eq!(report.funding_received, dec!(30));
        assert_eq!(report.funding_paid, dec!(8));
    }

    #[test]
    fn test_to_csv_contains_header() {
        let report = TaxReportGenerator::generate(Uuid::nil(), 2026, &[], &[]);
        let csv = TaxReportGenerator::to_csv(&report);
        assert!(csv.starts_with("User ID,Year,Total Trades"));
        // Two lines: header + data
        assert_eq!(csv.lines().count(), 2);
    }
}
