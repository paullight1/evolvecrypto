import { ApiClient } from "./apiClient";

export interface CryptoAsset {
    id: number;
    symbol: string;
    name: string;
    blockchain: string;
    contract_address?: string;
    decimals: number;
    is_active: boolean;
}

export interface FiatCurrency {
    id: number;
    code: string;
    name: string;
    symbol: string;
    is_active: boolean;
}

export interface PaymentMethod {
    id: number;
    code: string;
    name: string;
    category: "mobile_money" | "bank_transfer" | "online_wallet" | "crypto" | "cash";
    icon_url?: string;
    description?: string;
    is_active: boolean;
}

export class PlatformService {
    static async getCryptoAssets(): Promise<CryptoAsset[]> {
        const res = await ApiClient.get<{ crypto_assets: CryptoAsset[] }>("/api/v1/platform/crypto-assets?active_only=true");
        return res.crypto_assets || [];
    }

    static async getFiatCurrencies(): Promise<FiatCurrency[]> {
        const res = await ApiClient.get<{ fiat_currencies: FiatCurrency[] }>("/api/v1/platform/fiat-currencies?active_only=true");
        return res.fiat_currencies || [];
    }

    static async getPaymentMethods(): Promise<PaymentMethod[]> {
        const res = await ApiClient.get<{ payment_methods: PaymentMethod[] }>("/api/v1/platform/payment-methods?active_only=true");
        return res.payment_methods || [];
    }
}
