"use client";

import React from "react";
import { motion } from "framer-motion";
import {
    Clock,
    TrendingUp,
    TrendingDown,
    User,
    Star,
    ArrowRight
} from "lucide-react";

export interface TradeAd {
    ad_id: string;
    user_id: string;
    username: string;
    user_avatar?: string;
    is_online?: boolean;
    type: "buy" | "sell";
    crypto_symbol: string;
    fiat_symbol: string;
    price: number;
    min_amount: number;
    max_amount: number;
    payment_methods: string[];
    payment_method_ids: number[];
    payment_window_minutes: number;
    trade_terms?: string;
    status: "active" | "paused" | "closed";
    created_at: string;
    user_trades?: number;
    user_rating?: number;
}

interface AdCardProps {
    ad: TradeAd;
    onTrade: (adId: string) => void;
}

const AdCard: React.FC<AdCardProps> = ({ ad, onTrade }) => {
    const formatPrice = (ad: TradeAd) => {
        return `${ad.price.toLocaleString("en-US", { minimumFractionDigits: 2, maximumFractionDigits: 2 })}`;
    };

    const formatAmount = (amount: number) => {
        return amount.toLocaleString("en-US", { minimumFractionDigits: 0, maximumFractionDigits: 0 });
    };

    return (
        <motion.div
            initial={{ opacity: 0, y: 20 }}
            whileInView={{ opacity: 1, y: 0 }}
            viewport={{ once: true }}
            className="group relative"
        >
            {/* Background Glow */}
            <div className={`absolute -inset-0.5 rounded-[2.5rem] opacity-0 group-hover:opacity-100 transition-opacity duration-500 blur-xl -z-10 ${ad.type === 'buy' ? 'bg-primary/20' : 'bg-accent/20'
                }`} />

            <div className="glass relative h-full flex flex-col bg-white/[0.02] border border-white/5 rounded-[2.5rem] p-8 hover:bg-white/[0.04] hover:border-white/10 transition-all duration-500 overflow-hidden">
                {/* Type & Badge */}
                <div className="flex items-center justify-between mb-8">
                    <div className="flex items-center gap-3">
                        <div className={`px-4 py-1.5 rounded-xl text-[10px] font-black uppercase tracking-[0.2em] shadow-sm ${ad.type === "buy"
                            ? "bg-primary/10 text-primary border border-primary/20"
                            : "bg-accent/10 text-accent border border-accent/20"
                            }`}>
                            {ad.type === "buy" ? "Purchase" : "Liquidation"}
                        </div>
                        <div className="flex items-center gap-2 px-3 py-1.5 bg-white/5 rounded-xl border border-white/5">
                            <div className="w-1.5 h-1.5 rounded-full bg-white/20" />
                            <span className="text-[10px] font-black text-text-dim uppercase tracking-widest">{ad.crypto_symbol}</span>
                        </div>
                    </div>
                    {ad.is_online && (
                        <div className="flex items-center gap-2">
                            <span className="relative flex h-2 w-2">
                                <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-accent opacity-75"></span>
                                <span className="relative inline-flex rounded-full h-2 w-2 bg-accent"></span>
                            </span>
                            <span className="text-[9px] font-black text-accent uppercase tracking-widest">Active</span>
                        </div>
                    )}
                </div>

                {/* Price Section */}
                <div className="mb-8">
                    <p className="text-[10px] font-black text-text-dim/40 uppercase tracking-[0.2em] mb-3">Exchange Rate</p>
                    <div className="flex items-baseline gap-2">
                        <h2 className="text-4xl font-black text-white italic tracking-tighter">
                            {formatPrice(ad)}
                        </h2>
                        <span className="text-xs font-black text-text-dim uppercase tracking-widest">{ad.fiat_symbol}</span>
                    </div>
                    <div className="mt-4 p-4 rounded-2xl bg-white/[0.02] border border-white/5">
                        <div className="flex items-center justify-between">
                            <span className="text-[10px] font-black text-text-dim/40 uppercase tracking-widest">Order Limits</span>
                            <span className="text-[11px] font-black text-white italic">
                                {formatAmount(ad.min_amount)} - {formatAmount(ad.max_amount)} <span className="text-[9px] non-italic text-text-dim ml-1">{ad.fiat_symbol}</span>
                            </span>
                        </div>
                        <div className="mt-2 w-full h-1 bg-white/5 rounded-full overflow-hidden">
                            <div className="h-full w-2/3 bg-white/10 rounded-full" />
                        </div>
                    </div>
                </div>

                {/* Merchant Section */}
                <div className="flex items-center justify-between mb-8 p-1">
                    <div className="flex items-center gap-4">
                        <div className="relative">
                            <div className="w-12 h-12 rounded-2xl bg-gradient-to-br from-white/10 to-white/5 border border-white/10 flex items-center justify-center overflow-hidden transition-all duration-500 group-hover:scale-110">
                                {ad.user_avatar ? (
                                    <img src={ad.user_avatar} alt={ad.username} className="w-full h-full object-cover" />
                                ) : (
                                    <User className="w-6 h-6 text-white/20" />
                                )}
                            </div>
                        </div>
                        <div>
                            <p className="text-sm font-black text-white italic tracking-tight">{ad.username}</p>
                            <div className="flex items-center gap-3 mt-1">
                                <div className="flex items-center gap-1.5 px-2 py-0.5 bg-amber-500/10 rounded-lg">
                                    <Star className="w-3 h-3 text-amber-500 fill-amber-500" />
                                    <span className="text-[9px] font-black text-amber-500">
                                        {ad.user_rating !== undefined && ad.user_rating !== null
                                            ? ad.user_rating.toFixed(2)
                                            : (ad.user_trades && ad.user_trades > 0 ? "5.00" : "0.00")}
                                    </span>
                                </div>
                                <span className="text-[10px] font-black text-text-dim/40 uppercase tracking-widest">
                                    {ad.user_trades || 0} Trades
                                </span>
                            </div>
                        </div>
                    </div>
                    <div className="flex flex-col items-end gap-1">
                        <p className="text-[9px] font-black text-text-dim/30 uppercase tracking-widest">Time Window</p>
                        <div className="flex items-center gap-1.5 text-white/60">
                            <Clock className="w-3.5 h-3.5" />
                            <span className="text-[10px] font-black">{ad.payment_window_minutes} Min</span>
                        </div>
                    </div>
                </div>

                {/* Footnotes & Action */}
                <div className="mt-auto space-y-6">
                    <div className="flex flex-wrap gap-2">
                        {(ad.payment_methods || []).map((method) => (
                            <span key={method} className="px-3 py-1.5 bg-white/5 border border-white/5 rounded-xl text-[9px] font-black text-text-dim/80 uppercase tracking-widest">
                                {method}
                            </span>
                        ))}
                    </div>

                    <button
                        onClick={() => onTrade(ad.ad_id)}
                        className="relative w-full group/btn overflow-hidden rounded-2xl p-[1px] transition-all hover:scale-[1.02] active:scale-[0.98]"
                    >
                        <div className="absolute inset-0 bg-gradient-to-r from-primary via-secondary to-primary bg-[length:200%_100%] animate-gradient" />
                        <div className="relative flex items-center justify-center gap-3 w-full bg-background rounded-[15px] py-4 transition-all group-hover/btn:bg-transparent">
                            <span className="text-xs font-black text-white uppercase tracking-[0.2em] group-hover/btn:scale-105 transition-transform">Initialize Trade</span>
                            <ArrowRight className="w-4 h-4 text-white transition-transform group-hover/btn:translate-x-1" />
                        </div>
                    </button>
                </div>
            </div>
        </motion.div>
    );
};

export default AdCard;
