export interface WalletAsset {
  asset: string;
  total: string;
  available: string;
  locked: string;
  usd_value: string;
}

export interface WalletOverview {
  total_equity_usd: string;
  assets: WalletAsset[];
}
