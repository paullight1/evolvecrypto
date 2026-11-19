//! Weighted-median computation for TradFi price aggregation.
//!
//! This is the same algorithm used by `exchange-index-price`. It is duplicated
//! here so the TradFi oracle crate stays self-contained and independent of the
//! crypto index-price crate's internal API.

use rust_decimal::Decimal;
use rust_decimal_macros::dec;

/// Compute the weighted median of a slice of `(price, weight)` pairs.
///
/// The weighted median is the price `p` such that the total weight below `p`
/// is strictly less than 0.5 and the total weight above `p` is strictly less
/// than 0.5 (accounting for the weight at `p` itself).
///
/// Algorithm:
/// 1. Sort ascending by price.
/// 2. Accumulate weights from the bottom.
/// 3. The first price whose running cumulative weight (including itself) reaches
///    or exceeds 0.5 is the weighted median.
///
/// Returns `None` for an empty input.
pub fn weighted_median(values: &[(Decimal, Decimal)]) -> Option<Decimal> {
    if values.is_empty() {
        return None;
    }

    if values.len() == 1 {
        return Some(values[0].0);
    }

    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));

    let total_weight: Decimal = sorted.iter().map(|(_, w)| *w).sum();
    if total_weight == Decimal::ZERO {
        let mid = sorted.len() / 2;
        return Some(sorted[mid].0);
    }

    let half = total_weight / dec!(2);

    let mut cumulative = Decimal::ZERO;
    for (price, weight) in &sorted {
        cumulative += weight;
        if cumulative >= half {
            return Some(*price);
        }
    }

    Some(sorted.last().expect("sorted is non-empty").0)
}

#[cfg(test)]
mod tests {
    use rust_decimal_macros::dec;

    use super::*;

    #[test]
    fn empty_returns_none() {
        assert_eq!(weighted_median(&[]), None);
    }

    #[test]
    fn single_element_returns_its_price() {
        assert_eq!(weighted_median(&[(dec!(2350), dec!(1))]), Some(dec!(2350)));
    }

    #[test]
    fn equal_weights_two_elements_picks_lower() {
        let result = weighted_median(&[(dec!(1.085), dec!(0.5)), (dec!(1.090), dec!(0.5))]);
        assert_eq!(result, Some(dec!(1.085)));
    }

    #[test]
    fn heavy_high_weight_picks_upper() {
        let result = weighted_median(&[(dec!(1.085), dec!(0.2)), (dec!(1.090), dec!(0.8))]);
        assert_eq!(result, Some(dec!(1.090)));
    }

    #[test]
    fn three_sources_typical_case() {
        // PythSim 0.5, ForexFeed 0.3, CmdtyFeed 0.2; prices 2349, 2350, 2352.
        // Sorted: (2349, 0.2), (2350, 0.5), (2352, 0.3). Half = 0.5.
        // Cum at 2349 = 0.2 < 0.5; cum at 2350 = 0.7 >= 0.5 → 2350.
        let result = weighted_median(&[
            (dec!(2349), dec!(0.2)),
            (dec!(2350), dec!(0.5)),
            (dec!(2352), dec!(0.3)),
        ]);
        assert_eq!(result, Some(dec!(2350)));
    }

    #[test]
    fn extreme_outlier_does_not_shift_median() {
        // One feed wildly wrong at 9999; others near 2350. Outlier weight 0.1.
        let result = weighted_median(&[
            (dec!(2350), dec!(0.5)),
            (dec!(2351), dec!(0.3)),
            (dec!(2349), dec!(0.1)),
            (dec!(9999), dec!(0.1)),
        ]);
        assert!(result.unwrap() < dec!(3000));
    }

    #[test]
    fn unnormalised_weights_still_correct() {
        // Weights 3, 1 → half = 2; cum at 100 = 3 >= 2 → pick 100.
        let result = weighted_median(&[(dec!(100), dec!(3)), (dec!(200), dec!(1))]);
        assert_eq!(result, Some(dec!(100)));
    }
}
