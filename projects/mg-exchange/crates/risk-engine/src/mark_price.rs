//! Mark price calculation for perpetual futures.
//!
//! Mark price is the fair value used for unrealised PnL and liquidation checks.
//! It is anchored to the spot index to prevent short-term manipulation via the
//! last traded price on the exchange.

use exchange_common::{
    futures::MarkPrice,
    types::{Price, Symbol},
};
use rust_decimal::Decimal;
use chrono::Utc;

/// Default EMA span for the smoothed-basis mark price calculation.
///
/// A span of 30 samples at 1-minute sampling ≈ a ~30-minute half-life,
/// consistent with Deribit/Bybit basis-EMA conventions.
pub const DEFAULT_MARK_EMA_SPAN: u32 = 30;

/// Calculate mark price from index, last price, and a basis value.
///
/// Uses the simple formula:
/// `mark = (2 * index_price + last_price) / 3`
///
/// This caps the divergence from spot: the last price alone can move mark price
/// by at most ±33% of the spread, making single-exchange manipulation costly.
///
/// The `funding_rate` parameter is accepted for interface completeness and future
/// use in basis-EMA calculations but is not used in this simple version.
pub fn calculate_mark_price(
    symbol: Symbol,
    index_price: Price,
    last_price: Price,
    _funding_rate: Decimal,
) -> MarkPrice {
    let mark = (Decimal::TWO * index_price + last_price) / Decimal::from(3u32);
    MarkPrice {
        symbol,
        price: mark,
        index_price,
        last_price,
        timestamp: Utc::now(),
    }
}

/// Calculate mark price using an EMA-smoothed basis.
///
/// This variant applies an exponential moving average to the futures/spot basis
/// rather than using the instantaneous spread, reducing susceptibility to
/// single-tick manipulation.
///
/// # Formula
/// ```text
/// alpha     = 2 / (span + 1)
/// new_ema   = raw_basis * alpha + prev_ema * (1 - alpha)
/// mark      = index_price * (1 + new_ema)
/// ```
///
/// Returns `(MarkPrice, new_ema_basis)` so the caller can persist the updated EMA.
///
/// When `index_price` is zero the basis cannot be computed; in that case the
/// previous EMA is preserved and the mark price equals the index price.
pub fn calculate_mark_price_ema(
    symbol: Symbol,
    index_price: Price,
    last_price: Price,
    prev_ema: Decimal,
    span: u32,
) -> (MarkPrice, Decimal) {
    if index_price.is_zero() {
        let mp = MarkPrice {
            symbol,
            price: index_price,
            index_price,
            last_price,
            timestamp: Utc::now(),
        };
        return (mp, prev_ema);
    }

    let span_dec = Decimal::from(span.max(1));
    let alpha = Decimal::TWO / (span_dec + Decimal::ONE);
    let raw_basis = (last_price - index_price) / index_price;
    let new_ema = raw_basis * alpha + prev_ema * (Decimal::ONE - alpha);
    let mark = index_price * (Decimal::ONE + new_ema);

    let mp = MarkPrice {
        symbol,
        price: mark,
        index_price,
        last_price,
        timestamp: Utc::now(),
    };
    (mp, new_ema)
}

/// Compute the basis as a fraction: `(last_price - index_price) / index_price`.
///
/// Positive basis → futures premium. Negative → futures discount.
/// Returns `None` if `index_price` is zero (avoids division by zero).
pub fn basis_rate(index_price: Price, last_price: Price) -> Option<Decimal> {
    if index_price.is_zero() {
        return None;
    }
    Some((last_price - index_price) / index_price)
}

#[cfg(test)]
mod tests {
    use super::*;
    use exchange_common::types::Symbol;
    use rust_decimal_macros::dec;

    fn sym() -> Symbol {
        Symbol::new("BTCUSDT")
    }

    // --- calculate_mark_price ---

    #[test]
    fn test_mark_price_equal_index_and_last() {
        // When index == last, mark == both
        let mp = calculate_mark_price(sym(), dec!(50000), dec!(50000), dec!(0));
        assert_eq!(mp.price, dec!(50000));
        assert_eq!(mp.index_price, dec!(50000));
        assert_eq!(mp.last_price, dec!(50000));
    }

    #[test]
    fn test_mark_price_last_above_index() {
        // index 50_000, last 50_300 → mark = (100_000 + 50_300) / 3 = 50_100
        let mp = calculate_mark_price(sym(), dec!(50000), dec!(50300), dec!(0));
        assert_eq!(mp.price, dec!(50100));
    }

    #[test]
    fn test_mark_price_last_below_index() {
        // index 50_000, last 49_700 → mark = (100_000 + 49_700) / 3 = 49_900
        let mp = calculate_mark_price(sym(), dec!(50000), dec!(49700), dec!(0));
        assert_eq!(mp.price, dec!(49900));
    }

    #[test]
    fn test_mark_price_clamped_toward_index() {
        // Even with extreme last price, mark stays closer to index.
        // index 50_000, last 60_000 → mark = (100_000 + 60_000) / 3 = 53_333.33...
        let mp = calculate_mark_price(sym(), dec!(50000), dec!(60000), dec!(0));
        // mark is between index and last
        assert!(mp.price > dec!(50000));
        assert!(mp.price < dec!(60000));
    }

    #[test]
    fn test_mark_price_symbol_preserved() {
        let mp = calculate_mark_price(Symbol::new("ETHUSDT"), dec!(3000), dec!(3010), dec!(0));
        assert_eq!(mp.symbol, Symbol::new("ETHUSDT"));
    }

    // --- basis_rate ---

    #[test]
    fn test_basis_rate_positive() {
        // last > index → positive basis (futures premium)
        let b = basis_rate(dec!(50000), dec!(50500)).unwrap();
        assert_eq!(b, dec!(0.01)); // 1%
    }

    #[test]
    fn test_basis_rate_negative() {
        // last < index → negative basis (futures discount)
        let b = basis_rate(dec!(50000), dec!(49500)).unwrap();
        assert_eq!(b, dec!(-0.01));
    }

    #[test]
    fn test_basis_rate_zero_when_equal() {
        let b = basis_rate(dec!(50000), dec!(50000)).unwrap();
        assert_eq!(b, Decimal::ZERO);
    }

    #[test]
    fn test_basis_rate_none_on_zero_index() {
        assert!(basis_rate(dec!(0), dec!(50000)).is_none());
    }
}
