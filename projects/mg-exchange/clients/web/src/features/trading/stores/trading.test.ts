import { describe, it, expect, beforeEach } from 'vitest';
import { useTradingStore } from './trading';

function resetStore(): void {
  useTradingStore.setState({
    orderType: 'limit',
    side: 'buy',
    price: '',
    qty: '',
    stopPrice: '',
    leverage: 1,
    marginMode: 'cross',
    tpPrice: '',
    slPrice: '',
    reduceOnly: false,
    showTpSl: false,
    oneClickMode: false,
  });
}

describe('useTradingStore — initial state', () => {
  beforeEach(resetStore);

  it('default side is buy', () => {
    expect(useTradingStore.getState().side).toBe('buy');
  });

  it('default orderType is limit', () => {
    expect(useTradingStore.getState().orderType).toBe('limit');
  });

  it('default price is empty string', () => {
    expect(useTradingStore.getState().price).toBe('');
  });

  it('default qty is empty string', () => {
    expect(useTradingStore.getState().qty).toBe('');
  });

  it('default leverage is 1', () => {
    expect(useTradingStore.getState().leverage).toBe(1);
  });

  it('default marginMode is cross', () => {
    expect(useTradingStore.getState().marginMode).toBe('cross');
  });

  it('default reduceOnly is false', () => {
    expect(useTradingStore.getState().reduceOnly).toBe(false);
  });

  it('default showTpSl is false', () => {
    expect(useTradingStore.getState().showTpSl).toBe(false);
  });
});

describe('useTradingStore — setters', () => {
  beforeEach(resetStore);

  it('setSide changes side to sell', () => {
    useTradingStore.getState().setSide('sell');
    expect(useTradingStore.getState().side).toBe('sell');
  });

  it('setSide changes side back to buy', () => {
    useTradingStore.getState().setSide('sell');
    useTradingStore.getState().setSide('buy');
    expect(useTradingStore.getState().side).toBe('buy');
  });

  it('setPrice updates price', () => {
    useTradingStore.getState().setPrice('65000');
    expect(useTradingStore.getState().price).toBe('65000');
  });

  it('setQty updates qty', () => {
    useTradingStore.getState().setQty('0.5');
    expect(useTradingStore.getState().qty).toBe('0.5');
  });

  it('setOrderType updates orderType', () => {
    useTradingStore.getState().setOrderType('market');
    expect(useTradingStore.getState().orderType).toBe('market');
  });

  it('setStopPrice updates stopPrice', () => {
    useTradingStore.getState().setStopPrice('64000');
    expect(useTradingStore.getState().stopPrice).toBe('64000');
  });

  it('setLeverage updates leverage', () => {
    useTradingStore.getState().setLeverage(10);
    expect(useTradingStore.getState().leverage).toBe(10);
  });

  it('setMarginMode updates marginMode to isolated', () => {
    useTradingStore.getState().setMarginMode('isolated');
    expect(useTradingStore.getState().marginMode).toBe('isolated');
  });

  it('setTpPrice updates tpPrice', () => {
    useTradingStore.getState().setTpPrice('70000');
    expect(useTradingStore.getState().tpPrice).toBe('70000');
  });

  it('setSlPrice updates slPrice', () => {
    useTradingStore.getState().setSlPrice('60000');
    expect(useTradingStore.getState().slPrice).toBe('60000');
  });

  it('setReduceOnly sets reduceOnly to true', () => {
    useTradingStore.getState().setReduceOnly(true);
    expect(useTradingStore.getState().reduceOnly).toBe(true);
  });

  it('toggleShowTpSl flips showTpSl from false to true', () => {
    useTradingStore.getState().toggleShowTpSl();
    expect(useTradingStore.getState().showTpSl).toBe(true);
  });

  it('toggleShowTpSl flips showTpSl from true to false', () => {
    useTradingStore.setState({ showTpSl: true });
    useTradingStore.getState().toggleShowTpSl();
    expect(useTradingStore.getState().showTpSl).toBe(false);
  });

  it('fillFromOrderbook sets price', () => {
    useTradingStore.getState().fillFromOrderbook('49999');
    expect(useTradingStore.getState().price).toBe('49999');
  });

  it('fillQtyFromOrderbook sets qty', () => {
    useTradingStore.getState().fillQtyFromOrderbook('1.25');
    expect(useTradingStore.getState().qty).toBe('1.25');
  });
});

describe('useTradingStore — reset', () => {
  beforeEach(resetStore);

  it('clears price', () => {
    useTradingStore.getState().setPrice('65000');
    useTradingStore.getState().reset();
    expect(useTradingStore.getState().price).toBe('');
  });

  it('clears qty', () => {
    useTradingStore.getState().setQty('1.0');
    useTradingStore.getState().reset();
    expect(useTradingStore.getState().qty).toBe('');
  });

  it('clears stopPrice', () => {
    useTradingStore.getState().setStopPrice('63000');
    useTradingStore.getState().reset();
    expect(useTradingStore.getState().stopPrice).toBe('');
  });

  it('clears tpPrice', () => {
    useTradingStore.getState().setTpPrice('70000');
    useTradingStore.getState().reset();
    expect(useTradingStore.getState().tpPrice).toBe('');
  });

  it('clears slPrice', () => {
    useTradingStore.getState().setSlPrice('60000');
    useTradingStore.getState().reset();
    expect(useTradingStore.getState().slPrice).toBe('');
  });

  it('does not reset side', () => {
    useTradingStore.getState().setSide('sell');
    useTradingStore.getState().reset();
    // reset() only clears price/qty/stopPrice/tpPrice/slPrice — not side
    expect(useTradingStore.getState().side).toBe('sell');
  });
});
