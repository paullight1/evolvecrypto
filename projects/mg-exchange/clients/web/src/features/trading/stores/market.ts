import { create } from 'zustand';

// Symbols that trade as perpetual futures (USDT-margined) on this exchange.
const PERPETUAL_SYMBOLS = new Set([
  'BTCUSDT', 'ETHUSDT', 'SOLUSDT',
  'BNBUSDT', 'XRPUSDT', 'DOGEUSDT',
  'AVAXUSDT', 'ADAUSDT', 'MATICUSDT', 'LTCUSDT',
  'LINKUSDT', 'DOTUSDT', 'UNIUSDT', 'ATOMUSDT', 'NEARUSDT',
]);

// COIN-margined inverse perpetual symbols (e.g. BTCUSD, ETHUSD).
const COIN_M_SYMBOLS = new Set([
  'BTCUSD', 'ETHUSD', 'SOLUSD',
  'BNBUSD', 'XRPUSD', 'DOGEUSD',
]);

export type ProductType = 'spot' | 'perpetual' | 'coin_m';
export type TradingMode = 'spot' | 'futures' | 'coin_m';

export function getProductType(symbol: string): ProductType {
  if (COIN_M_SYMBOLS.has(symbol)) return 'coin_m';
  if (PERPETUAL_SYMBOLS.has(symbol)) return 'perpetual';
  return 'spot';
}

function convertSymbolForMode(symbol: string, mode: TradingMode): string {
  if (mode === 'spot') {
    if (symbol.includes('-')) return symbol;
    if (COIN_M_SYMBOLS.has(symbol)) {
      const base = symbol.slice(0, -3);
      return `${base}-USDT`;
    }
    const quote = symbol.endsWith('USDC') ? 'USDC' : 'USDT';
    const base = symbol.slice(0, symbol.length - quote.length);
    return `${base}-${quote}`;
  }
  if (mode === 'futures') {
    if (symbol.includes('-')) return symbol.replace('-', '');
    if (COIN_M_SYMBOLS.has(symbol)) return `${symbol.slice(0, -3)}USDT`;
    return symbol;
  }
  if (mode === 'coin_m') {
    if (COIN_M_SYMBOLS.has(symbol)) return symbol;
    const base = symbol.includes('-')
      ? symbol.split('-')[0]
      : symbol.endsWith('USDT') ? symbol.slice(0, -4) : symbol.slice(0, -4);
    const candidate = `${base}USD`;
    return COIN_M_SYMBOLS.has(candidate) ? candidate : 'BTCUSD';
  }
  return symbol;
}

export interface MarketState {
  symbol: string;
  productType: ProductType;
  tradingMode: TradingMode;
  setSymbol: (s: string) => void;
  setTradingMode: (mode: TradingMode) => void;

  // Ticker — common
  lastPrice: string;
  prevPrice: string;
  priceDirection: 'up' | 'down' | 'flat';
  priceChange24h: string;
  priceChangePct24h: string;
  high24h: string;
  low24h: string;
  volume24h: string;

  // Ticker — spread (enriched orderbook)
  spreadPct: string;

  // Ticker — perpetual only
  fundingRate: string;
  nextFundingTime: number;
  markPrice: string;
  indexPrice: string;
  openInterest: string;

  // Staleness tracking
  lastTickerUpdateTime: number;

  updateTicker: (
    data: Partial<
      Omit<
        MarketState,
        | 'symbol'
        | 'productType'
        | 'tradingMode'
        | 'setSymbol'
        | 'setTradingMode'
        | 'updateTicker'
        | 'lastTickerUpdateTime'
      >
    >,
  ) => void;
}

export const useMarketStore = create<MarketState>((set) => ({
  symbol: 'BTCUSDT',
  productType: 'perpetual',
  tradingMode: 'futures',
  setSymbol: (s) => set({ symbol: s, productType: getProductType(s) }),
  setTradingMode: (mode) =>
    set((state) => {
      const newSymbol = convertSymbolForMode(state.symbol, mode);
      return {
        tradingMode: mode,
        symbol: newSymbol,
        productType: getProductType(newSymbol),
      };
    }),

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

  updateTicker: (data) =>
    set((state) => {
      const newPrice = data.lastPrice ?? state.lastPrice;
      let direction: 'up' | 'down' | 'flat' = 'flat';
      if (newPrice && state.lastPrice) {
        const n = parseFloat(newPrice);
        const o = parseFloat(state.lastPrice);
        if (n > o) direction = 'up';
        else if (n < o) direction = 'down';
        else direction = 'flat';
      }
      return {
        ...data,
        prevPrice: state.lastPrice,
        priceDirection: newPrice !== state.lastPrice ? direction : state.priceDirection,
        lastTickerUpdateTime: Date.now(),
      };
    }),
}));
