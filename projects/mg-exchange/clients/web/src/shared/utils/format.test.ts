import { describe, it, expect } from 'vitest';
import { formatPrice, formatQty, formatUsd, formatPercent, cn } from './format';

describe('formatPrice', () => {
  it('formats to 2 decimals by default', () => {
    expect(formatPrice(65432.1)).toBe('65,432.10');
  });

  it('handles string input', () => {
    expect(formatPrice('65432.1')).toBe('65,432.10');
  });

  it('handles zero', () => {
    expect(formatPrice(0)).toBe('0.00');
  });

  it('formats integer with trailing zeros', () => {
    expect(formatPrice(100)).toBe('100.00');
  });

  it('formats with custom decimals', () => {
    expect(formatPrice(1234.5678, 4)).toBe('1,234.5678');
  });

  it('adds thousands separator', () => {
    expect(formatPrice(1000000)).toBe('1,000,000.00');
  });

  it('returns em dash for non-finite number string', () => {
    expect(formatPrice('not-a-number')).toBe('—');
  });

  it('returns em dash for Infinity', () => {
    expect(formatPrice(Infinity)).toBe('—');
  });

  it('returns em dash for NaN input', () => {
    expect(formatPrice(NaN)).toBe('—');
  });

  it('formats small decimal', () => {
    expect(formatPrice('0.50')).toBe('0.50');
  });
});

describe('formatQty', () => {
  it('formats whole number without trailing zeros', () => {
    expect(formatQty(1)).toBe('1');
  });

  it('formats decimal up to 4 places', () => {
    expect(formatQty('0.1234')).toBe('0.1234');
  });

  it('trims trailing zeros in fractional part', () => {
    // maximumFractionDigits=4 but minimumFractionDigits=0, so trailing zeros are stripped
    expect(formatQty('1.5000')).toBe('1.5');
  });

  it('handles zero', () => {
    expect(formatQty(0)).toBe('0');
  });

  it('returns em dash for non-finite input', () => {
    expect(formatQty('abc')).toBe('—');
  });

  it('formats large quantity with thousands separator', () => {
    expect(formatQty(10000)).toBe('10,000');
  });

  it('respects custom decimals argument', () => {
    expect(formatQty('1.23456789', 8)).toBe('1.23456789');
  });
});

describe('formatUsd', () => {
  it('adds $ prefix and formats to 2 decimals', () => {
    expect(formatUsd(1234.5)).toBe('$1,234.50');
  });

  it('handles string input', () => {
    expect(formatUsd('999.99')).toBe('$999.99');
  });

  it('handles zero', () => {
    expect(formatUsd(0)).toBe('$0.00');
  });

  it('formats large value with comma separators', () => {
    expect(formatUsd(1000000)).toBe('$1,000,000.00');
  });

  it('returns em dash for non-finite input', () => {
    expect(formatUsd('bad')).toBe('—');
  });

  it('handles negative values', () => {
    expect(formatUsd(-500)).toBe('-$500.00');
  });
});

describe('formatPercent', () => {
  it('adds % suffix for positive value', () => {
    expect(formatPercent(1.5)).toBe('+1.50%');
  });

  it('handles negative value without + sign', () => {
    expect(formatPercent(-2.75)).toBe('-2.75%');
  });

  it('handles zero', () => {
    expect(formatPercent(0)).toBe('0.00%');
  });

  it('handles string input', () => {
    expect(formatPercent('3.14')).toBe('+3.14%');
  });

  it('respects custom decimal places', () => {
    expect(formatPercent(1.23456, 4)).toBe('+1.2346%');
  });

  it('returns em dash for non-finite input', () => {
    expect(formatPercent('xyz')).toBe('—');
  });
});

// === Additional edge case tests ===

describe('formatPrice — edge cases', () => {
  it('preserves 8 decimal places when decimals=8', () => {
    // 0.00000001 with 8 decimals — toLocaleString must not strip trailing zeros
    expect(formatPrice('0.00000001', 8)).toBe('0.00000001');
  });

  it('handles very large numbers with correct comma separators', () => {
    expect(formatPrice('999999999.99')).toBe('999,999,999.99');
  });

  it('returns em dash for empty string input', () => {
    // parseFloat('') === NaN → !isFinite(NaN) === true
    expect(formatPrice('')).toBe('—');
  });

  it('returns em dash for string "Infinity"', () => {
    // parseFloat('Infinity') === Infinity → !isFinite(Infinity) === true
    expect(formatPrice('Infinity')).toBe('—');
  });
});

describe('formatQty — edge cases', () => {
  it('does not show trailing zeros beyond precision (min is 0)', () => {
    // minimumFractionDigits=0, maximumFractionDigits=4 → trailing zeros are stripped
    // '1.0000' formats as '1', not '1.0000'
    expect(formatQty('1.0000', 4)).toBe('1');
  });

  it('formats 1.5 with decimals=4 as "1.5" (no padding)', () => {
    // minimumFractionDigits=0 means no zero padding
    expect(formatQty('1.5', 4)).toBe('1.5');
  });

  it('formats value with exactly 4 meaningful decimals when precision=4', () => {
    expect(formatQty('1.2345', 4)).toBe('1.2345');
  });
});

describe('formatUsd — edge cases', () => {
  it('handles negative string values with $ prefix', () => {
    expect(formatUsd('-1234.56')).toBe('-$1,234.56');
  });
});

describe('formatPercent — edge cases', () => {
  it('shows + prefix for positive string value', () => {
    expect(formatPercent('5.5')).toMatch(/^\+5\.50%$/);
  });

  it('shows - prefix for negative string value', () => {
    expect(formatPercent('-3.2')).toMatch(/^-3\.20%$/);
  });
});

describe('cn', () => {
  it('joins class names with space', () => {
    expect(cn('foo', 'bar')).toBe('foo bar');
  });

  it('filters out false values', () => {
    expect(cn('foo', false, 'bar')).toBe('foo bar');
  });

  it('filters out undefined', () => {
    expect(cn('foo', undefined, 'bar')).toBe('foo bar');
  });

  it('filters out null', () => {
    expect(cn('foo', null, 'bar')).toBe('foo bar');
  });

  it('returns empty string when all values are falsy', () => {
    expect(cn(false, undefined, null)).toBe('');
  });

  it('returns single class when given one string', () => {
    expect(cn('only')).toBe('only');
  });

  it('handles empty string argument as falsy-ish — it passes through', () => {
    // empty string is falsy, gets filtered out by Boolean
    expect(cn('a', '', 'b')).toBe('a b');
  });
});
