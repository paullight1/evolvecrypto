import { describe, it, expect, beforeEach } from 'vitest';
import { useMarketStore, getProductType } from './market';

function resetStore(): void {
  useMarketStore.setState({
    symbol: 'BTCUSDT',
    productType: 'perpetual',
    tradingMode: 'futures',
    lastPrice: '',
    prevPrice: '',
    priceDirection: 'flat',
    priceChange24h: '',
    priceChangePct24h: '',
    high24h: '',
    low24h: '',
    volume24h: '',
    spreadPct: '',
    fundingRate: '',
    nextFundingTime: 0,
    markPrice: '',
    indexPrice: '',
    openInterest: '',
    lastTickerUpdateTime: 0,
  });
}

describe('getProductType', () => {
  it('returns "perpetual" for BTCUSDT', () => {
    expect(getProductType('BTCUSDT')).toBe('perpetual');
  });

  it('returns "coin_m" for BTCUSD', () => {
    expect(getProductType('BTCUSD')).toBe('coin_m');
  });

  it('returns "spot" for unknown symbol', () => {
    expect(getProductType('UNKNOWN')).toBe('spot');
  });

  it('returns "perpetual" for ETHUSDT', () => {
    expect(getProductType('ETHUSDT')).toBe('perpetual');
  });

  it('returns "coin_m" for ETHUSD', () => {
    expect(getProductType('ETHUSD')).toBe('coin_m');
  });
});

describe('useMarketStore — initial state', () => {
  beforeEach(resetStore);

  it('default symbol is BTCUSDT', () => {
    expect(useMarketStore.getState().symbol).toBe('BTCUSDT');
  });

  it('default productType is perpetual', () => {
    expect(useMarketStore.getState().productType).toBe('perpetual');
  });

  it('default tradingMode is futures', () => {
    expect(useMarketStore.getState().tradingMode).toBe('futures');
  });

  it('default priceDirection is flat', () => {
    expect(useMarketStore.getState().priceDirection).toBe('flat');
  });
});

describe('useMarketStore — setSymbol', () => {
  beforeEach(resetStore);

  it('updates symbol', () => {
    useMarketStore.getState().setSymbol('ETHUSDT');
    expect(useMarketStore.getState().symbol).toBe('ETHUSDT');
  });

  it('updates productType based on new symbol', () => {
    useMarketStore.getState().setSymbol('BTCUSD');
    expect(useMarketStore.getState().productType).toBe('coin_m');
  });

  it('sets productType to spot for unknown symbol', () => {
    useMarketStore.getState().setSymbol('XYZABC');
    expect(useMarketStore.getState().productType).toBe('spot');
  });
});

describe('useMarketStore — updateTicker', () => {
  beforeEach(resetStore);

  it('updates lastPrice', () => {
    useMarketStore.getState().updateTicker({ lastPrice: '65000' });
    expect(useMarketStore.getState().lastPrice).toBe('65000');
  });

  it('sets prevPrice to the old lastPrice', () => {
    useMarketStore.setState({ lastPrice: '64000' });
    useMarketStore.getState().updateTicker({ lastPrice: '65000' });
    expect(useMarketStore.getState().prevPrice).toBe('64000');
  });

  it('sets priceDirection to "up" when price increases', () => {
    useMarketStore.setState({ lastPrice: '64000' });
    useMarketStore.getState().updateTicker({ lastPrice: '65000' });
    expect(useMarketStore.getState().priceDirection).toBe('up');
  });

  it('sets priceDirection to "down" when price decreases', () => {
    useMarketStore.setState({ lastPrice: '65000' });
    useMarketStore.getState().updateTicker({ lastPrice: '64000' });
    expect(useMarketStore.getState().priceDirection).toBe('down');
  });

  it('sets priceDirection to "flat" when price is unchanged', () => {
    useMarketStore.setState({ lastPrice: '65000' });
    useMarketStore.getState().updateTicker({ lastPrice: '65000' });
    expect(useMarketStore.getState().priceDirection).toBe('flat');
  });

  it('updates fundingRate', () => {
    useMarketStore.getState().updateTicker({ fundingRate: '0.0001' });
    expect(useMarketStore.getState().fundingRate).toBe('0.0001');
  });

  it('updates volume24h', () => {
    useMarketStore.getState().updateTicker({ volume24h: '12345.67' });
    expect(useMarketStore.getState().volume24h).toBe('12345.67');
  });

  it('updates lastTickerUpdateTime to a positive number', () => {
    const before = Date.now();
    useMarketStore.getState().updateTicker({ lastPrice: '1000' });
    expect(useMarketStore.getState().lastTickerUpdateTime).toBeGreaterThanOrEqual(before);
  });
});

describe('useMarketStore — setTradingMode', () => {
  beforeEach(resetStore);

  it('switching to spot from BTCUSDT converts symbol to BTC-USDT', () => {
    useMarketStore.getState().setTradingMode('spot');
    expect(useMarketStore.getState().symbol).toBe('BTC-USDT');
  });

  it('switching to coin_m from BTCUSDT converts to BTCUSD', () => {
    useMarketStore.getState().setTradingMode('coin_m');
    expect(useMarketStore.getState().symbol).toBe('BTCUSD');
  });

  it('sets tradingMode correctly', () => {
    useMarketStore.getState().setTradingMode('spot');
    expect(useMarketStore.getState().tradingMode).toBe('spot');
  });
});
