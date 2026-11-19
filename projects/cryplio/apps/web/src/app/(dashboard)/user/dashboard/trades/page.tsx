"use client";

import React, { useState, useEffect } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { toast } from "sonner";
import {
    Clock,
    CheckCircle2,
    XCircle,
    AlertTriangle,
    ChevronRight,
    Search,
    Filter,
    ArrowUpRight,
    ArrowDownLeft,
    Loader2,
    MessageSquare,
    ExternalLink
} from "lucide-react";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { TradeService } from "@/services/tradeService";
import DashboardLayout from "@/components/dashboard/DashboardLayout";

const UserTradesPage = () => {
    const router = useRouter();
    const [trades, setTrades] = useState<any[]>([]);
    const [isLoading, setIsLoading] = useState(true);
    const [filter, setFilter] = useState<"all" | "active" | "completed" | "cancelled">("all");
    const [searchQuery, setSearchQuery] = useState("");

    useEffect(() => {
        fetchTrades();
    }, []);

    const fetchTrades = async () => {
        setIsLoading(true);
        try {
            const data = await TradeService.getMyTrades();
            setTrades(data || []);
        } catch (err: any) {
            console.error(err.message);
        } finally {
            setIsLoading(false);
        }
    };

    const getStatusColor = (status: string) => {
        switch (status) {
            case "pending": return "text-yellow-500 bg-yellow-500/10 border-yellow-500/20";
            case "active": return "text-primary bg-primary/10 border-primary/20";
            case "paid": return "text-blue-500 bg-blue-500/10 border-blue-500/20";
            case "released": return "text-accent bg-accent/10 border-accent/20";
            case "completed": return "text-accent bg-accent/10 border-accent/20";
            case "cancelled": return "text-red-500 bg-red-500/10 border-red-500/20";
            case "expired": return "text-text-dim bg-white/5 border-white/10";
            default: return "text-text-dim bg-white/5 border-white/10";
        }
    };

    const filteredTrades = trades.filter(t => {
        const matchesFilter = filter === "all" ||
            (filter === "active" && ["pending", "active", "paid", "disputed"].includes(t.status)) ||
            (filter === "completed" && ["released", "completed"].includes(t.status)) ||
            (filter === "cancelled" && ["cancelled", "expired"].includes(t.status));

        const matchesSearch = t.trade_id.toLowerCase().includes(searchQuery.toLowerCase()) ||
            t.crypto_amount.toString().includes(searchQuery) ||
            t.fiat_amount.toString().includes(searchQuery);

        return matchesFilter && matchesSearch;
    });

    return (
        <DashboardLayout title="My Trades" role="user">
            <div className="space-y-8 pb-10">
                {/* Header & Filters */}
                <div className="flex flex-col lg:flex-row lg:items-end justify-between gap-8">
                    <div className="space-y-1">
                        <h1 className="text-4xl font-black text-white italic uppercase tracking-tighter">My <span className="gradient-text">Ledger</span></h1>
                        <p className="text-[10px] text-text-dim font-bold uppercase tracking-[0.2em] opacity-60">System Synchronized • Real-time Trade Monitoring</p>
                    </div>

                    <div className="flex flex-col md:flex-row items-center gap-4">
                        <div className="relative group w-full md:w-72">
                            <Search className="absolute left-4 top-1/2 -translate-y-1/2 w-4 h-4 text-text-dim group-focus-within:text-primary transition-colors" />
                            <input
                                type="text"
                                placeholder="Search by ID or amount..."
                                value={searchQuery}
                                onChange={(e) => setSearchQuery(e.target.value)}
                                className="w-full bg-white/5 border border-white/10 rounded-2xl py-3 pl-12 pr-4 text-xs font-bold text-white outline-none focus:border-primary/50 focus:bg-white/[0.08] transition-all placeholder:text-text-dim/30"
                            />
                        </div>
                        <div className="flex items-center gap-1 bg-white/5 p-1.5 rounded-[1.25rem] border border-white/5">
                            {["all", "active", "completed", "cancelled"].map((f) => (
                                <button
                                    key={f}
                                    onClick={() => setFilter(f as any)}
                                    className={`px-5 py-2.5 rounded-xl text-[9px] font-black uppercase tracking-widest transition-all ${filter === f ? "bg-white text-background shadow-lg" : "text-text-dim hover:text-white"}`}
                                >
                                    {f}
                                </button>
                            ))}
                        </div>
                    </div>
                </div>

                {/* Trades List */}
                <div className="space-y-4">
                    <AnimatePresence mode="wait">
                        {isLoading ? (
                            <motion.div
                                initial={{ opacity: 0 }}
                                animate={{ opacity: 1 }}
                                exit={{ opacity: 0 }}
                                className="flex flex-col items-center justify-center py-24 space-y-4"
                            >
                                <Loader2 className="w-10 h-10 text-primary animate-spin" />
                                <p className="text-[10px] font-black uppercase tracking-widest text-text-dim">Syncing Ledger...</p>
                            </motion.div>
                        ) : filteredTrades.length === 0 ? (
                            <motion.div
                                initial={{ opacity: 0, y: 10 }}
                                animate={{ opacity: 1, y: 0 }}
                                className="bg-surface border border-white/5 rounded-[2.5rem] p-20 text-center space-y-4"
                            >
                                <div className="w-16 h-16 bg-white/5 rounded-full flex items-center justify-center mx-auto">
                                    <Clock className="w-8 h-8 text-text-dim/20" />
                                </div>
                                <h3 className="text-xl font-black text-white italic uppercase tracking-tight">No Transactions Found</h3>
                                <p className="text-xs text-text-dim max-w-xs mx-auto">Your trade history will appear here once you initiate or respond to an offer.</p>
                                <Link
                                    href="/marketplace"
                                    className="inline-flex items-center px-6 py-3 bg-white text-background rounded-xl font-black uppercase tracking-widest text-[10px] hover:scale-105 transition-all mt-4"
                                >
                                    Explore Marketplace
                                </Link>
                            </motion.div>
                        ) : (
                            filteredTrades.map((trade, i) => (
                                <motion.div
                                    key={trade.trade_id}
                                    initial={{ opacity: 0, y: 20 }}
                                    animate={{ opacity: 1, y: 0 }}
                                    transition={{ delay: i * 0.05, ease: [0.23, 1, 0.32, 1] }}
                                    onClick={() => router.push(`/marketplace/trade/${trade.trade_id}`)}
                                    className="group relative"
                                >
                                    {/* Glass Container */}
                                    <div className="relative overflow-hidden bg-white/5 border border-white/5 rounded-[2rem] p-6 hover:bg-white/[0.08] hover:border-white/10 transition-all duration-300">
                                        <div className="flex flex-col lg:flex-row lg:items-center justify-between gap-6">

                                            {/* Primary Info */}
                                            <div className="flex items-center gap-6">
                                                <div className={`w-14 h-14 rounded-2xl flex items-center justify-center transition-transform group-hover:scale-105 duration-500 ${trade.type === 'buy' ? 'bg-primary/20 text-primary shadow-[0_0_20px_-5px_rgba(var(--primary-rgb),0.3)]' : 'bg-accent/20 text-accent shadow-[0_0_20px_-5px_rgba(var(--accent-rgb),0.3)]'
                                                    }`}>
                                                    {trade.type === 'buy' ? <ArrowUpRight className="w-6 h-6" /> : <ArrowDownLeft className="w-6 h-6" />}
                                                </div>

                                                <div>
                                                    <div className="flex items-center gap-3 mb-2">
                                                        <button
                                                            onClick={(e) => {
                                                                e.stopPropagation();
                                                                navigator.clipboard.writeText(trade.trade_id);
                                                                toast.success("Trade ID copied");
                                                            }}
                                                            className="text-[10px] font-black text-text-dim/40 uppercase tracking-[0.2em] hover:text-white transition-colors"
                                                        >
                                                            #{trade.trade_id.slice(0, 8)}
                                                        </button>
                                                        <span className={`px-3 py-1 rounded-full text-[9px] font-black uppercase tracking-widest border ${getStatusColor(trade.status)}`}>
                                                            {trade.status}
                                                        </span>
                                                    </div>
                                                    <div className="flex items-center gap-3">
                                                        <h3 className="text-2xl font-black text-white italic tracking-tight">{trade.crypto_amount.toFixed(4)} <span className="text-[11px] non-italic text-text-dim uppercase tracking-widest ml-1">{trade.crypto_symbol || 'USDT'}</span></h3>
                                                        <span className="text-white/10 font-thin text-3xl">/</span>
                                                        <h4 className="text-xl font-black text-white/70 italic tracking-tight">{trade.fiat_amount.toLocaleString()} <span className="text-[10px] non-italic text-text-dim/60 uppercase tracking-widest ml-1">{trade.fiat_symbol || 'USD'}</span></h4>
                                                    </div>
                                                </div>
                                            </div>

                                            {/* Time & Counterparty */}
                                            <div className="flex items-center gap-10 md:px-6">
                                                <div className="space-y-1">
                                                    <p className="text-[9px] font-black text-text-dim/30 uppercase tracking-widest">Initialization</p>
                                                    <div className="flex items-center gap-2 text-text-dim">
                                                        <Clock className="w-3.5 h-3.5" />
                                                        <span className="text-[10px] font-bold uppercase tracking-widest">{new Date(trade.created_at).toLocaleString([], { dateStyle: 'short', timeStyle: 'short' })}</span>
                                                    </div>
                                                </div>
                                            </div>

                                            {/* Final Actions */}
                                            <div className="flex items-center gap-2">
                                                <Link
                                                    href={`/marketplace/trade/${trade.trade_id}`}
                                                    onClick={(e) => e.stopPropagation()}
                                                    className="flex items-center gap-2 px-6 py-3.5 rounded-2xl bg-primary text-white text-[11px] font-black uppercase tracking-[0.15em] hover:shadow-[0_10px_30px_-10px_rgba(var(--primary-rgb),0.5)] transition-all active:scale-95"
                                                >
                                                    Details
                                                    <ChevronRight className="w-4 h-4 group-hover:translate-x-1 transition-transform" />
                                                </Link>
                                            </div>
                                        </div>
                                    </div>
                                </motion.div>
                            ))
                        )}
                    </AnimatePresence>
                </div>
            </div>
        </DashboardLayout>
    );
};

export default UserTradesPage;
