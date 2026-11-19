"use client";

import React, { useState } from "react";
import { MarketplaceService, AdResponse } from "@/services/marketplaceService";
import {
    Clock,
    TrendingUp,
    TrendingDown,
    MoreVertical,
    Pause,
    Play,
    Trash2,
    Eye,
    Plus
} from "lucide-react";
import ConfirmModal from "@/components/ui/ConfirmModal";
import { motion, AnimatePresence } from "framer-motion";
import Link from "next/link";
import { toast } from "sonner";

interface MyListingsProps {
    ads: AdResponse[];
    onRefresh: () => void;
}

const MyListings = ({ ads, onRefresh }: MyListingsProps) => {
    const [isUpdating, setIsUpdating] = useState<string | null>(null);
    const [deleteAdId, setDeleteAdId] = useState<string | null>(null);

    const handleToggleStatus = async (adId: string) => {
        setIsUpdating(adId);
        try {
            await MarketplaceService.toggleAdStatus(adId);
            toast.success("Ad status updated");
            onRefresh();
        } catch (error) {
            toast.error("Failed to update status");
        } finally {
            setIsUpdating(null);
        }
    };

    const handleDeleteAd = async (adId: string) => {
        setIsUpdating(adId);
        try {
            await MarketplaceService.deleteAd(adId);
            toast.success("Ad deleted");
            onRefresh();
        } catch (error) {
            toast.error("Failed to delete ad");
        } finally {
            setIsUpdating(null);
            setDeleteAdId(null);
        }
    };

    if (ads.length === 0) {
        return (
            <div className="bg-surface rounded-[2.5rem] border border-white/10 p-8">
                <div className="flex items-center justify-between mb-8">
                    <h3 className="text-xl font-black text-white italic uppercase tracking-tight">My Active Listings</h3>
                </div>
                <div className="py-12 text-center border border-dashed border-white/10 rounded-3xl">
                    <Plus className="w-8 h-8 text-text-dim mx-auto mb-3 opacity-20" />
                    <p className="text-[10px] font-bold text-text-dim uppercase tracking-[0.2em] mb-4">No active advertisements</p>
                    <Link
                        href="/marketplace/create"
                        className="inline-flex px-6 py-3 bg-primary text-white rounded-xl font-black text-[10px] uppercase tracking-widest hover:scale-105 transition-all"
                    >
                        Create Your First Ad
                    </Link>
                </div>
            </div>
        );
    }

    return (
        <div className="bg-surface rounded-[2.5rem] border border-white/10 p-8">
            <div className="flex items-center justify-between mb-8">
                <h3 className="text-xl font-black text-white italic uppercase tracking-tight">Active Listings</h3>
                <Link href="/marketplace/create" className="text-xs font-black text-primary hover:text-white transition-all flex items-center uppercase tracking-widest">
                    <Plus className="w-4 h-4 mr-1" /> New Ad
                </Link>
            </div>

            <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                {ads.map((ad) => (
                    <div
                        key={ad.ad_id}
                        className="group relative bg-white/5 border border-white/5 hover:border-white/10 rounded-[2rem] p-6 transition-all"
                    >
                        <div className="flex items-center justify-between mb-4">
                            <div className="flex items-center space-x-2">
                                <div className={`px-2 py-0.5 rounded-lg text-[8px] font-black uppercase tracking-widest border ${ad.type === 'buy'
                                    ? 'bg-accent/10 text-accent border-accent/20'
                                    : 'bg-primary/10 text-primary border-primary/20'
                                    }`}>
                                    {ad.type === 'buy' ? 'BUYING' : 'SELLING'}
                                </div>
                                <span className="text-[10px] font-black text-white italic">{ad.crypto_symbol}</span>
                            </div>
                            <div className={`w-2 h-2 rounded-full ${(!ad.is_paused && ad.status === 'active') ? 'bg-accent animate-pulse' : 'bg-yellow-500'}`} title={ad.is_paused ? 'Paused' : ad.status} />
                        </div>

                        <div className="mb-4">
                            <div className="flex items-baseline space-x-2">
                                <span className="text-2xl font-black text-white italic">{ad.price.toLocaleString()}</span>
                                <span className="text-[10px] font-bold text-text-dim uppercase tracking-widest">{ad.fiat_symbol}</span>
                            </div>
                            <p className="text-[10px] font-bold text-text-dim uppercase tracking-widest mt-1 opacity-60">
                                {ad.min_amount.toLocaleString()} - {ad.max_amount.toLocaleString()} {ad.fiat_symbol}
                            </p>
                        </div>

                        <div className="flex items-center gap-2 pt-4 border-t border-white/5">
                            <button
                                onClick={() => handleToggleStatus(ad.ad_id)}
                                disabled={isUpdating === ad.ad_id}
                                className="flex-1 py-3 bg-white/5 hover:bg-white/10 text-white rounded-xl text-[10px] font-black uppercase tracking-widest transition-all flex items-center justify-center gap-2 disabled:opacity-50"
                            >
                                {!ad.is_paused ? (
                                    <><Pause className="w-3 h-3" /> Pause</>
                                ) : (
                                    <><Play className="w-3 h-3" /> Resume</>
                                )}
                            </button>
                            <button
                                onClick={() => setDeleteAdId(ad.ad_id)}
                                disabled={isUpdating === ad.ad_id}
                                className="px-4 py-3 bg-red-500/10 hover:bg-red-500/20 text-red-500 rounded-xl transition-all disabled:opacity-50"
                            >
                                <Trash2 className="w-3 h-3" />
                            </button>
                        </div>
                    </div>
                ))}
            </div>

            <ConfirmModal
                open={!!deleteAdId}
                title="Delete Advertisement"
                description="Are you sure you want to delete this advertisement? This action cannot be undone."
                confirmText={isUpdating ? "Deleting..." : "Delete"}
                isDestructive
                onClose={() => setDeleteAdId(null)}
                onConfirm={() => deleteAdId && handleDeleteAd(deleteAdId)}
            />
        </div>
    );
};

export default MyListings;
