import { get, post } from '@shared/api/client';
import type {
  Chain,
  DexQuote,
  BridgeRoute,
  BridgeQuote,
  Web3Wallet,
  TokenBalance,
  GetDexQuoteRequest,
  ExecuteSwapRequest,
  GetBridgeRoutesRequest,
  GetBridgeQuoteRequest,
  ExecuteBridgeRequest,
} from './types';

export const getDexChains = (): Promise<Chain[]> =>
  get<Chain[]>('/dex/chains');

export const getDexQuote = (req: GetDexQuoteRequest): Promise<DexQuote> =>
  post<DexQuote>('/dex/quote', req);

export const executeDexSwap = (req: ExecuteSwapRequest): Promise<{ tx_hash: string }> =>
  post<{ tx_hash: string }>('/dex/swap', req);

export const getSmartRoute = (req: GetDexQuoteRequest): Promise<DexQuote> =>
  post<DexQuote>('/dex/route', req);

export const createWeb3Wallet = (chain: string): Promise<Web3Wallet> =>
  post<Web3Wallet>('/web3/wallet', { chain });

export const getWeb3Balances = (): Promise<TokenBalance[]> =>
  get<TokenBalance[]>('/web3/wallet');

export const getBridgeRoutes = (req: GetBridgeRoutesRequest): Promise<BridgeRoute[]> =>
  post<BridgeRoute[]>('/web3/bridge/routes', req);

export const getBridgeQuote = (req: GetBridgeQuoteRequest): Promise<BridgeQuote> =>
  post<BridgeQuote>('/web3/bridge/quote', req);

export const executeBridge = (req: ExecuteBridgeRequest): Promise<{ tx_hash: string }> =>
  post<{ tx_hash: string }>('/web3/bridge/execute', req);
