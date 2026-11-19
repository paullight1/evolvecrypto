"use client";

import React, { useState, useEffect } from "react";
import { useParams, useRouter } from "next/navigation";
import {
    Shield,
    Clock,
    ArrowLeft,
    Loader2,
    CheckCircle2,
    Zap,
    Activity,
    Star,
    DollarSign,
    ShieldCheck,
    Info,
    User
} from "lucide-react";
import { motion, AnimatePresence } from "framer-motion";
import { toast } from "sonner";
import { useAuth } from "@/context/AuthContext";
import Navbar from "@/components/layout/Navbar";
import Footer from "@/components/layout/Footer";
import { MarketplaceService, AdResponse } from "@/services/marketplaceService";
import { TradeService } from "@/services/tradeService";

const AdDetailsPage = () => {
    const { id } = useParams();
    const router = useRouter();
    const { user: authUser, isLoading: authLoading } = useAuth();

    const [ad, setAd] = useState<AdResponse | null>(null);
    const [isLoading, setIsLoading] = useState(true);
    const [tradeAmount, setTradeAmount] = useState("");
    const [isInitiating, setIsInitiating] = useState(false);
    const [initiationError, setInitiationError] = useState<string | null>(null);

    useEffect(() => {
        const fetchAd = async () => {
            try {
                setIsLoading(true);
                const data = await MarketplaceService.getAdDetails(id as string);
                setAd(data);
                setTradeAmount(data.min_amount.toString());
            } catch (err: any) {
                toast.error(err.message || "Failed to load advertisement");
            } finally {
                setIsLoading(false);
            }
        };

        if (id) {
            fetchAd();
        }
    }, [id]);

    const handleInitiateTrade = async () => {
        if (authLoading || !ad) return;

        if (!authUser) {
            toast.error("Please login to trade");
            router.push(`/login?redirect=${encodeURIComponent(window.location.pathname)}`);
            return;
        }

        if (authUser.id === ad.user_id) {
            toast.error("You cannot trade with your own advertisement");
            return;
        }

        setIsInitiating(true);
        setInitiationError(null);
        try {
            const amount = parseFloat(tradeAmount);
            if (isNaN(amount) || amount < ad.min_amount) {
                throw new Error(`Minimum amount is ${ad.min_amount} ${ad.fiat_symbol}`);
            }
            if (amount > ad.max_amount) {
                throw new Error(`Maximum amount is ${ad.max_amount} ${ad.fiat_symbol}`);
            }
            // Use the first payment method ID if available, otherwise 0
            const pmId = ad.payment_method_ids ? ad.payment_method_ids[0] : 0;
            const result = await TradeService.initiateTrade(ad.ad_id, amount, pmId);
            toast.success("Trade initiated successfully!");
            router.push(`/user/dashboard/trades/${result.trade_id}`);
        } catch (err: any) {
            setInitiationError(err.message);
            toast.error(err.message);
        } finally {
            setIsInitiating(false);
        }
    };

    if (isLoading) {
        return (
            <div className="min-h-screen bg-background flex flex-col items-center justify-center">
                <Loader2 className="w-12 h-12 text-primary animate-spin mb-4" />
                <p className="text-text-dim font-black uppercase tracking-widest text-[10px] animate-pulse">Loading Ad Details...</p>
            </div>
        );
    }

    if (!ad) {
        return (
            <div className="min-h-screen bg-background flex flex-col items-center justify-center p-6 text-center">
                <h1 className="text-3xl font-black text-white mb-2 italic">AD NOT FOUND</h1>
                <p className="text-text-dim max-w-md mb-8">The advertisement you are looking for does not exist or has been removed.</p>
                <button onClick={() => router.push("/marketplace")} className="px-8 py-4 bg-primary text-white rounded-xl font-black uppercase tracking-widest text-xs">Back to Marketplace</button>
            </div>
        );
    }

    return (
        <main className="min-h-screen bg-background relative overflow-hidden">
            <Navbar />

            {/* Background Decorative Elements */}
            <div className="absolute top-0 left-1/4 w-[500px] h-[500px] bg-primary/5 rounded-full blur-[120px] -z-10 animate-pulse" />
            <div className="absolute bottom-0 right-1/4 w-[500px] h-[500px] bg-secondary/5 rounded-full blur-[120px] -z-10 animate-pulse" />

            <div className="pt-40 pb-32">
                <div className="container mx-auto px-6 md:px-10">

                    <motion.button
                        initial={{ opacity: 0, x: -10 }}
                        animate={{ opacity: 1, x: 0 }}
                        onClick={() => router.back()}
                        className="flex items-center text-text-dim/60 hover:text-white transition-all mb-12 group"
                    >
                        <div className="w-10 h-10 rounded-xl bg-white/5 border border-white/5 flex items-center justify-center mr-4 group-hover:bg-white/10 group-hover:border-white/10 transition-all">
                            <ArrowLeft className="w-5 h-5 group-hover:-translate-x-1 transition-transform" />
                        </div>
                        <span className="text-[10px] font-black uppercase tracking-[0.3em]">Return to Nexus</span>
                    </motion.button>

                    <div className="grid grid-cols-1 lg:grid-cols-12 gap-16 items-start">

                        {/* LEFT: AD DETAILS */}
                        <div className="lg:col-span-12">
                            <div className="flex flex-col lg:flex-row lg:items-center justify-between gap-12 mb-16">
                                <div className="space-y-4">
                                    <div className="flex items-center gap-4">
                                        <div className={`px-4 py-1.5 rounded-xl text-[10px] font-black uppercase tracking-[0.3em] shadow-2xl ${ad.type === 'buy' ? 'bg-primary/10 text-primary border border-primary/20' : 'bg-accent/10 text-accent border border-accent/20'
                                            }`}>
                                            Protocol Type: {ad.type}
                                        </div>
                                        <div className="px-4 py-1.5 bg-white/5 border border-white/10 rounded-xl text-[10px] font-black text-text-dim uppercase tracking-[0.3em]">
                                            Live Execution
                                        </div>
                                    </div>
                                    <h1 className="text-7xl font-black text-white italic uppercase tracking-tighter leading-none">
                                        {ad.crypto_symbol} <span className="text-white/20">/</span> {ad.fiat_symbol}
                                    </h1>
                                    <div className="flex items-center gap-6">
                                        <div className="flex items-baseline gap-2">
                                            <span className="text-3xl font-black text-white italic tracking-tight">{ad.price.toLocaleString()}</span>
                                            <span className="text-sm font-black text-text-dim uppercase tracking-widest opacity-40">{ad.fiat_symbol}</span>
                                        </div>
                                        <div className="w-[1px] h-6 bg-white/10" />
                                        <p className="text-[10px] font-black text-text-dim/40 uppercase tracking-[0.3em]">Real-time Settlement Rate</p>
                                    </div>
                                </div>

                                <div className="flex items-center gap-8 p-8 glass rounded-[3rem] border-white/5 bg-white/[0.02] hover:bg-white/[0.04] transition-all duration-500 group/merchant">
                                    <div className="relative">
                                        <div className="w-24 h-24 rounded-[2rem] bg-gradient-to-br from-white/10 to-white/5 border border-white/10 flex items-center justify-center overflow-hidden shadow-2xl transition-transform duration-500 group-hover/merchant:scale-110">
                                            {ad.user_avatar ? (
                                                <img
                                                    src={ad.user_avatar}
                                                    alt={ad.username}
                                                    className="w-full h-full object-cover"
                                                    onError={(e) => {
                                                        (e.target as HTMLImageElement).src = `https://api.dicebear.com/7.x/avataaars/svg?seed=${ad.username}`;
                                                    }}
                                                />
                                            ) : (
                                                <User className="w-10 h-10 text-white/20" />
                                            )}
                                        </div>
                                        {ad.is_online && (
                                            <div className="absolute -top-1 -right-1 w-5 h-5 bg-accent border-[4px] border-background rounded-full shadow-lg" />
                                        )}
                                    </div>
                                    <div>
                                        <div className="flex items-center gap-3 mb-2">
                                            <p className="text-[10px] font-black text-text-dim/40 uppercase tracking-[0.3em]">Verified Integrator</p>
                                            <ShieldCheck className="w-4 h-4 text-accent" />
                                        </div>
                                        <h4 className="text-3xl font-black text-white hover:text-primary transition-colors cursor-pointer italic uppercase tracking-tighter" onClick={() => router.push(`/u/${ad.username}`)}>
                                            {ad.username}
                                        </h4>
                                        <div className="flex flex-wrap items-center gap-4 mt-6">
                                            <div className="flex items-center text-amber-500 gap-2 px-4 py-1.5 bg-amber-500/10 rounded-xl border border-amber-500/20">
                                                <Star className="w-3.5 h-3.5 fill-amber-500" />
                                                <span className="text-[10px] font-black uppercase tracking-widest">
                                                    {ad.user_rating !== undefined && ad.user_rating !== null
                                                        ? ad.user_rating.toFixed(2)
                                                        : (ad.user_trades && ad.user_trades > 0 ? "5.00" : "0.00")}
                                                </span>
                                            </div>
                                            <div className="flex items-center text-primary/80 gap-2 px-4 py-1.5 bg-primary/10 rounded-xl border border-primary/20">
                                                <Activity className="w-3.5 h-3.5" />
                                                <span className="text-[10px] font-black uppercase tracking-widest">{ad.user_trades || 0} Transactions</span>
                                            </div>
                                        </div>
                                    </div>
                                </div>
                            </div>
                        </div>

                        {/* MIDDLE: INFO */}
                        <div className="lg:col-span-7 space-y-12">
                            <section className="glass rounded-[3.5rem] p-12 border-white/5 bg-white/[0.01]">
                                <div className="flex items-center justify-between mb-12">
                                    <h3 className="text-2xl font-black text-white italic uppercase tracking-tighter flex items-center gap-4">
                                        <Info className="w-7 h-7 text-primary" /> Protocol Parameters
                                    </h3>
                                    <div className="px-4 py-1 bg-white/5 rounded-full text-[9px] font-black text-text-dim/40 uppercase tracking-widest">System Scan: Valid</div>
                                </div>

                                <div className="grid grid-cols-1 md:grid-cols-2 gap-12">
                                    <div className="space-y-2">
                                        <p className="text-[10px] font-black text-text-dim/40 uppercase tracking-[0.2em]">Execution Window</p>
                                        <div className="flex items-center gap-3">
                                            <Clock className="w-5 h-5 text-white/20" />
                                            <p className="text-lg font-black text-white italic">{ad.payment_window_minutes} Standards Minutes</p>
                                        </div>
                                    </div>
                                    <div className="space-y-2">
                                        <p className="text-[10px] font-black text-text-dim/40 uppercase tracking-[0.2em]">Capacity Limits</p>
                                        <div className="flex items-center gap-3">
                                            <DollarSign className="w-5 h-5 text-white/20" />
                                            <p className="text-lg font-black text-white italic">{ad.min_amount.toLocaleString()} - {ad.max_amount.toLocaleString()} <span className="text-xs non-italic text-text-dim opacity-40 ml-1">{ad.fiat_symbol}</span></p>
                                        </div>
                                    </div>
                                    <div className="space-y-4 md:col-span-2">
                                        <p className="text-[10px] font-black text-text-dim/40 uppercase tracking-[0.2em]">Accepted Settlement Channels</p>
                                        <div className="flex flex-wrap gap-3">
                                            {ad.payment_methods.map((pm, idx) => (
                                                <span key={idx} className="px-6 py-3 bg-white/5 border border-white/10 rounded-2xl text-[10px] font-black text-white/80 uppercase tracking-widest hover:bg-white/10 transition-colors cursor-default">{pm}</span>
                                            ))}
                                        </div>
                                    </div>
                                </div>

                                <div className="mt-12 pt-12 border-t border-white/5">
                                    <p className="text-[10px] font-black text-text-dim/40 uppercase tracking-[0.3em] mb-6">Manifesto & Terms</p>
                                    <div className="p-8 bg-white/[0.02] rounded-[2.5rem] border border-white/5">
                                        <p className="text-sm text-text-dim/80 leading-relaxed italic font-medium whitespace-pre-wrap">
                                            {ad.trade_terms || "Integrator has not specified custom manifesting parameters for this execution."}
                                        </p>
                                    </div>
                                </div>
                            </section>

                            <section className="p-12 glass rounded-[3.5rem] bg-accent/5 border-accent/10 flex flex-col md:flex-row items-center gap-10">
                                <div className="w-24 h-24 rounded-[2rem] bg-accent/10 flex items-center justify-center border border-accent/20 shadow-2xl shadow-accent/20 animate-pulse">
                                    <ShieldCheck className="w-12 h-12 text-accent" />
                                </div>
                                <div className="flex-1 text-center md:text-left">
                                    <h4 className="text-2xl font-black text-white italic tracking-tighter uppercase mb-2">Protocol Secured Escrow</h4>
                                    <p className="text-xs text-text-dim font-bold leading-relaxed uppercase tracking-[0.2em] opacity-60">
                                        All assets are locked in the Cryplio smart-contract gateway until mutual settlement is confirmed.
                                        Your integrity is mathematically guaranteed.
                                    </p>
                                </div>
                            </section>
                        </div>

                        {/* RIGHT: INITIATE */}
                        <div className="lg:col-span-5 sticky top-32">
                            <motion.div
                                initial={{ opacity: 0, y: 20 }}
                                animate={{ opacity: 1, y: 0 }}
                                className="glass rounded-[4rem] p-12 border-white/10 shadow-[0_50px_100px_-20px_rgba(0,0,0,0.5)] relative overflow-hidden group/card"
                            >
                                <div className="absolute top-0 right-0 p-12 overflow-hidden">
                                    <Activity className="w-16 h-16 text-primary/5 -rotate-12 group-hover/card:rotate-0 transition-transform duration-1000" />
                                </div>

                                <h3 className="text-3xl font-black text-white mb-3 italic uppercase tracking-tighter">Initialize <span className="gradient-text">Exchange</span></h3>
                                <p className="text-text-dim/40 text-[10px] font-black uppercase tracking-[0.3em] mb-12">System Calibration Required</p>

                                <div className="space-y-10">
                                    <div>
                                        <div className="flex justify-between items-end mb-6 px-4">
                                            <label className="text-[10px] font-black text-text-dim/60 uppercase tracking-[0.3em]">{ad.type === "sell" ? "Input Fiat Volume" : "Output Fiat Volume"}</label>
                                            <p className="text-[10px] font-black text-primary uppercase tracking-widest">AVAILABLE CAPACITY</p>
                                        </div>
                                        <div className="relative group">
                                            <input
                                                type="number"
                                                value={tradeAmount}
                                                onChange={(e) => setTradeAmount(e.target.value)}
                                                className="w-full bg-white/[0.03] border-2 border-white/5 rounded-[2.5rem] py-10 px-12 text-4xl font-black text-white focus:border-primary/50 focus:bg-white/[0.06] outline-none transition-all pr-32 italic tracking-tighter"
                                                placeholder="0.00"
                                            />
                                            <div className="absolute right-12 top-1/2 -translate-y-1/2 font-black text-text-dim/20 text-2xl italic uppercase">{ad.fiat_symbol}</div>
                                        </div>

                                        <div className="mt-10 p-8 rounded-[2.5rem] bg-primary/5 border border-primary/10 relative overflow-hidden group/zap">
                                            <div className="absolute top-0 right-0 p-4 opacity-10 group-hover/zap:scale-150 transition-transform duration-700">
                                                <Zap className="w-12 h-12 text-primary" />
                                            </div>
                                            <div className="flex items-center justify-between relative z-10">
                                                <p className="text-[10px] font-black text-text-dim uppercase tracking-[0.2em]">Projected Asset Yield</p>
                                                <div className="flex items-baseline gap-2">
                                                    <span className="text-2xl font-black text-white italic tracking-tight">{(parseFloat(tradeAmount || "0") / ad.price).toFixed(8)}</span>
                                                    <span className="text-[10px] font-black text-primary uppercase tracking-widest">{ad.crypto_symbol}</span>
                                                </div>
                                            </div>
                                        </div>
                                    </div>

                                    {initiationError && (
                                        <motion.div
                                            initial={{ opacity: 0, height: 0 }}
                                            animate={{ opacity: 1, height: 'auto' }}
                                            className="p-6 bg-red-500/10 border border-red-500/20 rounded-3xl"
                                        >
                                            <p className="text-[10px] font-bold text-red-500 uppercase tracking-widest text-center">{initiationError}</p>
                                        </motion.div>
                                    )}

                                    <button
                                        onClick={handleInitiateTrade}
                                        disabled={isInitiating}
                                        className="relative w-full group/btn overflow-hidden rounded-[2.5rem] p-[1px] transition-all hover:scale-[1.02] active:scale-[0.98]"
                                    >
                                        <div className="absolute inset-0 bg-gradient-to-r from-primary via-secondary to-primary bg-[length:200%_100%] animate-gradient" />
                                        <div className="relative flex flex-col items-center justify-center gap-1 w-full bg-background rounded-[39px] py-8 transition-all group-hover/btn:bg-transparent">
                                            {isInitiating ? <Loader2 className="w-6 h-6 animate-spin" /> : (
                                                <>
                                                    <span className="text-sm font-black text-white uppercase tracking-[0.3em]">{ad.type === "buy" ? "Execute Liquidation" : "Execute Acquisition"}</span>
                                                    <span className="text-[8px] font-black text-white/30 uppercase tracking-[0.5em] group-hover/btn:text-white/60">Instant Escrow Activation</span>
                                                </>
                                            )}
                                        </div>
                                    </button>

                                    <div className="flex justify-center gap-8">
                                        <div className="flex flex-col items-center gap-2">
                                            <Shield className="w-4 h-4 text-accent/40" />
                                            <span className="text-[8px] font-black text-text-dim/40 uppercase tracking-widest">Secured</span>
                                        </div>
                                        <div className="flex flex-col items-center gap-2">
                                            <CheckCircle2 className="w-4 h-4 text-accent/40" />
                                            <span className="text-[8px] font-black text-text-dim/40 uppercase tracking-widest">Verified</span>
                                        </div>
                                        <div className="flex flex-col items-center gap-2">
                                            <Zap className="w-4 h-4 text-accent/40" />
                                            <span className="text-[8px] font-black text-text-dim/40 uppercase tracking-widest">Instant</span>
                                        </div>
                                    </div>
                                </div>
                            </motion.div>
                        </div>
                    </div>
                </div>
            </div>

            <Footer />
        </main>
    );
};

export default AdDetailsPage;
