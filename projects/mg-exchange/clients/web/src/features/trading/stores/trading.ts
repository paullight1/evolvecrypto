import { create } from 'zustand';
import type { OrderType } from '@shared/api/types';

export type TimeInForce = 'gtc' | 'ioc' | 'fok' | 'post_only';

export interface TradingState {
  orderType: OrderType;
  side: 'buy' | 'sell';
  price: string;
  qty: string;
  stopPrice: string;
  leverage: number;
  marginMode: 'cross' | 'isolated';
  tpPrice: string;
  slPrice: string;
  reduceOnly: boolean;
  showTpSl: boolean;
  oneClickMode: boolean;
  timeInForce: TimeInForce;
  trailingDelta: string;
  trailingType: 'fixed' | 'percentage';

  setOrderType: (t: OrderType) => void;
  setSide: (s: 'buy' | 'sell') => void;
  setPrice: (p: string) => void;
  setQty: (q: string) => void;
  setStopPrice: (p: string) => void;
  setLeverage: (l: number) => void;
  setMarginMode: (m: 'cross' | 'isolated') => void;
  setTpPrice: (p: string) => void;
  setSlPrice: (p: string) => void;
  setReduceOnly: (v: boolean) => void;
  toggleShowTpSl: () => void;
  setOneClickMode: (v: boolean) => void;
  setTimeInForce: (t: TimeInForce) => void;
  setTrailingDelta: (d: string) => void;
  setTrailingType: (t: 'fixed' | 'percentage') => void;
  fillFromOrderbook: (price: string) => void;
  fillQtyFromOrderbook: (qty: string) => void;
  reset: () => void;
}

export const useTradingStore = create<TradingState>((set) => ({
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
  oneClickMode: localStorage.getItem('one_click_mode') === '1',
  timeInForce: 'gtc',
  trailingDelta: '',
  trailingType: 'fixed',

  setOrderType: (t) => set({ orderType: t }),
  setSide: (s) => set({ side: s }),
  setPrice: (p) => set({ price: p }),
  setQty: (q) => set({ qty: q }),
  setStopPrice: (p) => set({ stopPrice: p }),
  setLeverage: (l) => set({ leverage: l }),
  setMarginMode: (m) => set({ marginMode: m }),
  setTpPrice: (p) => set({ tpPrice: p }),
  setSlPrice: (p) => set({ slPrice: p }),
  setReduceOnly: (v) => set({ reduceOnly: v }),
  toggleShowTpSl: () => set((s) => ({ showTpSl: !s.showTpSl })),
  setOneClickMode: (v) => {
    localStorage.setItem('one_click_mode', v ? '1' : '0');
    set({ oneClickMode: v });
  },
  setTimeInForce: (t) => set({ timeInForce: t }),
  setTrailingDelta: (d) => set({ trailingDelta: d }),
  setTrailingType: (t) => set({ trailingType: t }),
  fillFromOrderbook: (price) => set({ price }),
  fillQtyFromOrderbook: (qty) => set({ qty }),
  reset: () => set({ price: '', qty: '', stopPrice: '', tpPrice: '', slPrice: '', trailingDelta: '' }),
}));
