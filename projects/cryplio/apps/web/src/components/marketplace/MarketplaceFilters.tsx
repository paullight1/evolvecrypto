"use client";

import React, { useState, useEffect } from "react";
import { Search, Filter, ChevronDown } from "lucide-react";
import { PlatformService, FiatCurrency, PaymentMethod } from "@/services/platformService";

interface MarketplaceFiltersProps {
    searchTerm: string;
    setSearchTerm: (val: string) => void;
    filters: {
        type: string;
        fiat_currency: string;
        payment_method: string;
        sort_by: string;
    };
    setFilters: (filters: any) => void;
    showFilters: boolean;
    setShowFilters: (show: boolean) => void;
}

const MarketplaceFilters: React.FC<MarketplaceFiltersProps> = ({
    searchTerm,
    setSearchTerm,
    filters,
    setFilters,
    showFilters,
    setShowFilters
}) => {
    const [fiats, setFiats] = useState<FiatCurrency[]>([]);
    const [methods, setMethods] = useState<PaymentMethod[]>([]);

    useEffect(() => {
        const loadMetadata = async () => {
            try {
                const [fiatData, methodData] = await Promise.all([
                    PlatformService.getFiatCurrencies(),
                    PlatformService.getPaymentMethods()
                ]);
                setFiats(fiatData);
                setMethods(methodData);
            } catch (error) {
                console.error("Failed to load filter metadata:", error);
            }
        };
        loadMetadata();
    }, []);

    const filterConfigs = [
        {
            label: "Operation Type",
            key: "type",
            options: [
                { v: "all", l: "Unified Stream" },
                { v: "buy", l: "Buy Operations" },
                { v: "sell", l: "Sell Operations" }
            ]
        },
        {
            label: "Settlement Asset",
            key: "fiat_currency",
            options: [
                { v: "all", l: "Multi-Currency" },
                ...fiats.map(f => ({ v: f.code, l: `${f.code} (${f.name})` }))
            ]
        },
        {
            label: "Clearance Method",
            key: "payment_method",
            options: [
                { v: "all", l: "All Gateways" },
                ...methods.map(m => ({ v: m.code, l: m.name }))
            ]
        },
        {
            label: "Prioritization",
            key: "sort_by",
            options: [
                { v: "best_price", l: "Optimal Yield" },
                { v: "completion_rate", l: "High Reliability" },
                { v: "newest", l: "Latest Entries" },
                { v: "trade_count", l: "Volume Leaders" }
            ]
        },
    ];

    return (
        <div className="glass border-white/5 rounded-[2rem] p-8 mb-12">
            <div className="flex flex-col lg:flex-row gap-6">
                {/* Search */}
                <div className="flex-1 relative group">
                    <Search className="absolute left-6 top-1/2 -translate-y-1/2 w-5 h-5 text-text-dim/40 group-focus-within:text-primary transition-colors" />
                    <input
                        type="text"
                        placeholder="Search specific nodes or protocols..."
                        value={searchTerm}
                        onChange={(e) => setSearchTerm(e.target.value)}
                        className="w-full pl-16 pr-8 py-5 bg-white/[0.03] border border-white/5 rounded-2xl text-sm font-black uppercase tracking-widest text-white placeholder-text-dim/30 focus:outline-none focus:border-primary/30 focus:bg-white/[0.05] transition-all"
                    />
                </div>

                {/* Filter Toggle */}
                <button
                    onClick={() => setShowFilters(!showFilters)}
                    className="px-8 py-5 bg-white/[0.03] border border-white/5 rounded-2xl text-[10px] font-black uppercase tracking-[0.2em] text-white/60 hover:text-white hover:bg-white/[0.06] transition-all flex items-center justify-center gap-4 group"
                >
                    <Filter className="w-4 h-4 opacity-40 group-hover:opacity-100 transition-opacity" />
                    Protocol Configuration
                    <ChevronDown className={`w-4 h-4 transition-transform duration-500 ${showFilters ? "rotate-180" : ""}`} />
                </button>
            </div>

            {/* Advanced Filters */}
            {showFilters && (
                <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-8 mt-10 pt-10 border-t border-white/5">
                    {filterConfigs.map((f) => (
                        <div key={f.key} className="space-y-4">
                            <label className="block text-[9px] font-black text-text-dim/40 uppercase tracking-[0.4em] italic">{f.label}</label>
                            <div className="relative group/sel">
                                <select
                                    value={(filters as any)[f.key]}
                                    onChange={(e) => setFilters({ ...filters, [f.key]: e.target.value })}
                                    className="w-full appearance-none px-6 py-4 bg-white/[0.03] border border-white/5 rounded-xl text-[11px] font-black text-white hover:bg-white/[0.06] transition-all focus:outline-none focus:border-primary/20 uppercase tracking-widest cursor-pointer"
                                >
                                    {f.options.map(opt => <option key={opt.v} value={opt.v} className="bg-background text-white">{opt.l}</option>)}
                                </select>
                                <ChevronDown className="absolute right-4 top-1/2 -translate-y-1/2 w-4 h-4 text-white/20 pointer-events-none group-hover/sel:text-white/40 transition-colors" />
                            </div>
                        </div>
                    ))}
                </div>
            )}
        </div>
    );
};

export default MarketplaceFilters;

