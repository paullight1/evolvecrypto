use rust_decimal::Decimal;
use rust_decimal_macros::dec;

/// Compute the weighted median of a slice of `(price, weight)` pairs.
///
/// The weighted median is the price `p` such that the sum of weights for prices
/// below `p` is less than 0.5 and the sum for prices above `p` is less than 0.5.
///
/// Algorithm:
/// 1. Sort ascending by price.
/// 2. Accumulate weights from the bottom.
/// 3. The first price whose *running cumulative weight* (including itself) reaches
///    or exceeds 0.5 is the weighted median.
///
/// Returns `None` for an empty input.
pub fn weighted_median(values: &[(Decimal, Decimal)]) -> Option<Decimal> {
    if values.is_empty() {
        return None;
    }

    // Single element: trivial.
    if values.len() == 1 {
        return Some(values[0].0);
    }

    // Sort a local copy by price ascending.
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));

    // Total weight (normalise so the algorithm works even if weights don't sum to 1.0).
    let total_weight: Decimal = sorted.iter().map(|(_, w)| *w).sum();
    if total_weight == Decimal::ZERO {
        // All zero weights: fall back to simple median by count.
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

    // Fallback: return the highest price (shouldn't be reached with positive total weight).
    Some(sorted.last().unwrap().0)
}

#[cfg(test)]
mod tests {
    use rust_decimal_macros::dec;

    use super::*;

    // ── Basic correctness ───────────────────────────────────────────────────

    #[test]
    fn empty_returns_none() {
        assert_eq!(weighted_median(&[]), None);
    }

    #[test]
    fn single_element() {
        let result = weighted_median(&[(dec!(100), dec!(1))]);
        assert_eq!(result, Some(dec!(100)));
    }

    #[test]
    fn two_equal_weights_lower_median() {
        // With equal weights [0.5, 0.5] on prices [100, 200]:
        // cumulative at 100 → 0.5 which equals half → pick 100.
        let result = weighted_median(&[(dec!(100), dec!(0.5)), (dec!(200), dec!(0.5))]);
        assert_eq!(result, Some(dec!(100)));
    }

    #[test]
    fn two_elements_heavy_low() {
        // weight 0.7 on 100, weight 0.3 on 200 → cumulative at 100 reaches 0.7 ≥ 0.5 → 100
        let result = weighted_median(&[(dec!(100), dec!(0.7)), (dec!(200), dec!(0.3))]);
        assert_eq!(result, Some(dec!(100)));
    }

    #[test]
    fn two_elements_heavy_high() {
        // weight 0.3 on 100, weight 0.7 on 200 → cumulative at 100 = 0.3 < 0.5 → move on
        // cumulative at 200 = 1.0 ≥ 0.5 → pick 200
        let result = weighted_median(&[(dec!(100), dec!(0.3)), (dec!(200), dec!(0.7))]);
        assert_eq!(result, Some(dec!(200)));
    }

    #[test]
    fn three_elements_median_is_middle() {
        // Weights [0.33, 0.34, 0.33] on prices [90, 100, 110].
        // half ≈ 0.5. cumulative at 90 = 0.33 < 0.5; at 100 = 0.67 ≥ 0.5 → pick 100.
        let result = weighted_median(&[
            (dec!(90), dec!(0.33)),
            (dec!(100), dec!(0.34)),
            (dec!(110), dec!(0.33)),
        ]);
        assert_eq!(result, Some(dec!(100)));
    }

    // ── Outlier resistance ──────────────────────────────────────────────────

    #[test]
    fn single_extreme_outlier_does_not_shift_result() {
        // Binance/Coinbase/Kraken near 50 000; OKX has a crazy 200 000 outlier.
        // Total weight 1.0, half = 0.5.
        // Cumulative at 49800 = 0.20 < 0.5
        // Cumulative at 50000 = 0.50 ≥ 0.5 → pick 50000
        let result = weighted_median(&[
            (dec!(50000), dec!(0.40)), // Binance
            (dec!(50050), dec!(0.30)), // Coinbase
            (dec!(49800), dec!(0.20)), // Kraken
            (dec!(200000), dec!(0.10)), // OKX — extreme outlier
        ]);
        assert_eq!(result, Some(dec!(50000)));
    }

    #[test]
    fn low_outlier_does_not_shift_result() {
        // One source reports impossibly low price with small weight.
        let result = weighted_median(&[
            (dec!(100), dec!(0.40)),
            (dec!(101), dec!(0.30)),
            (dec!(99), dec!(0.20)),
            (dec!(1), dec!(0.10)), // outlier
        ]);
        // Sorted: (1, 0.10), (99, 0.20), (100, 0.40), (101, 0.30)
        // half = 0.5
        // cum at 1  = 0.10 < 0.5
        // cum at 99 = 0.30 < 0.5
        // cum at 100 = 0.70 ≥ 0.5 → pick 100
        assert_eq!(result, Some(dec!(100)));
    }

    // ── Edge cases ──────────────────────────────────────────────────────────

    #[test]
    fn equal_weights_odd_count() {
        // 5 equal-weight sources at 10,20,30,40,50. Median by weight = 30.
        let result = weighted_median(&[
            (dec!(10), dec!(0.2)),
            (dec!(20), dec!(0.2)),
            (dec!(30), dec!(0.2)),
            (dec!(40), dec!(0.2)),
            (dec!(50), dec!(0.2)),
        ]);
        assert_eq!(result, Some(dec!(30)));
    }

    #[test]
    fn unsorted_input_gives_correct_result() {
        // Same as equal_weights_odd_count but shuffled.
        let result = weighted_median(&[
            (dec!(50), dec!(0.2)),
            (dec!(10), dec!(0.2)),
            (dec!(40), dec!(0.2)),
            (dec!(20), dec!(0.2)),
            (dec!(30), dec!(0.2)),
        ]);
        assert_eq!(result, Some(dec!(30)));
    }

    #[test]
    fn zero_weight_sources_ignored() {
        // A zero-weight source at an extreme price must not shift the median.
        let result = weighted_median(&[
            (dec!(100), dec!(0.5)),
            (dec!(102), dec!(0.5)),
            (dec!(999999), dec!(0)), // zero weight — effectively absent
        ]);
        // half = 0.5. cum at 100 = 0.5 ≥ 0.5 → pick 100.
        assert_eq!(result, Some(dec!(100)));
    }

    #[test]
    fn all_same_price() {
        let result = weighted_median(&[
            (dec!(100), dec!(0.4)),
            (dec!(100), dec!(0.3)),
            (dec!(100), dec!(0.2)),
            (dec!(100), dec!(0.1)),
        ]);
        assert_eq!(result, Some(dec!(100)));
    }

    #[test]
    fn weights_not_normalised_still_correct() {
        // Weights don't need to sum to 1.0 — the algorithm normalises internally.
        // Equivalent to [100→2, 200→1], median at 100 (2/3 > 0.5).
        let result = weighted_median(&[(dec!(100), dec!(2)), (dec!(200), dec!(1))]);
        // half = 1.5. cum at 100 = 2.0 ≥ 1.5 → pick 100.
        assert_eq!(result, Some(dec!(100)));
    }
}
