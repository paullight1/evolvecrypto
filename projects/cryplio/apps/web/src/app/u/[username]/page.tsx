"use client";

import React, { useState, useEffect } from "react";
import { useParams, useRouter } from "next/navigation";
import {
    User,
    Shield,
    Calendar,
    Star,
    TrendingUp,
    CheckCircle2,
    MessageSquare,
    ShieldCheck,
    Ban,
    Loader2,
    ArrowLeft,
    Clock,
    Zap,
    ThumbsUp,
    ThumbsDown,
    Activity
} from "lucide-react";
import { motion, AnimatePresence } from "framer-motion";
import { toast } from "sonner";
import { authService, BackendUser, UserStats } from "@/services/authService";
import { MarketplaceService, AdResponse } from "@/services/marketplaceService";
import { useAuth } from "@/context/AuthContext";
import Navbar from "@/components/layout/Navbar";
import Footer from "@/components/layout/Footer";
import ConfirmModal from "@/components/ui/ConfirmModal";

const PublicProfilePage = () => {
    const { username } = useParams();
    const router = useRouter();
    const { user: currentUser } = useAuth();

    const [profile, setProfile] = useState<{ user: BackendUser; stats: UserStats } | null>(null);
    const [activeAds, setActiveAds] = useState<AdResponse[]>([]);
    const [isLoading, setIsLoading] = useState(true);
    const [error, setError] = useState<string | null>(null);


    useEffect(() => {
        const fetchProfileData = async () => {
            try {
                setIsLoading(true);
                const data = await authService.getUserByUsername(username as string);
                setProfile(data);

                // Fetch user's active ads
                if (data?.user?.id) {
                    const adsData = await MarketplaceService.getAds({ user_id: data.user.id, limit: "10" });
                    setActiveAds(adsData.ads || []);
                }
            } catch (err: any) {
                setError(err.message || "Failed to load profile");
            } finally {
                setIsLoading(false);
            }
        };

        if (username) {
            fetchProfileData();
        }
    }, [username]);



    if (isLoading) {
        return (
            <div className="min-h-screen bg-background flex flex-col items-center justify-center">
                <Loader2 className="w-12 h-12 text-primary animate-spin mb-4" />
                <p className="text-text-dim font-black uppercase tracking-widest text-xs animate-pulse">Loading Profile...</p>
            </div>
        );
    }

    if (error || !profile) {
        return (
            <div className="min-h-screen bg-background flex flex-col items-center justify-center p-6 text-center">
                <div className="w-20 h-20 rounded-3xl bg-red-500/10 flex items-center justify-center mb-6 border border-red-500/20">
                    <Ban className="w-10 h-10 text-red-500" />
                </div>
                <h1 className="text-3xl font-black text-white mb-2 uppercase tracking-tight">Profile Not Found</h1>
                <p className="text-text-dim max-w-md mb-8">{error || "The user you are looking for does not exist or has been deactivated."}</p>
                <button
                    onClick={() => router.push("/marketplace")}
                    className="px-8 py-4 bg-primary text-white rounded-2xl font-black uppercase tracking-widest text-xs hover:scale-105 transition-all shadow-xl shadow-primary/20"
                >
                    Back to Marketplace
                </button>
            </div>
        );
    }

    const { user, stats: rawStats } = profile;
    const stats = rawStats || {} as UserStats;
    const isOwnProfile = currentUser?.id === user.id;
    const joinDate = new Date(user.created_at || Date.now()).toLocaleDateString('en-US', {
        month: 'long',
        year: 'numeric'
    });

    const isOnline = user.is_online;

    const formatTime = (seconds: number) => {
        if (!seconds || seconds <= 0) return "N/A";
        if (seconds < 60) return `${seconds}s`;
        return `~${(seconds / 60).toFixed(1)} min`;
    };

    const getSpeedPercentage = (seconds: number) => {
        if (!seconds || seconds <= 0) return 0;
        const maxTime = 1800; // 30 mins
        return Math.max(5, 100 - (seconds / maxTime) * 100);
    };

    return (
        <main className="min-h-screen bg-background relative overflow-hidden">
            <Navbar />

            {/* Background Decorative Elements */}
            <div className="absolute top-0 right-1/4 w-[600px] h-[600px] bg-primary/5 rounded-full blur-[120px] -z-10 animate-pulse" />
            <div className="absolute bottom-0 left-1/4 w-[600px] h-[600px] bg-secondary/5 rounded-full blur-[120px] -z-10 animate-pulse" />

            <div className="pt-32 pb-32">
                <div className="container mx-auto px-6 md:px-10">

                    {/* Back Button */}
                    <motion.button
                        initial={{ opacity: 0, x: -10 }}
                        animate={{ opacity: 1, x: 0 }}
                        onClick={() => router.back()}
                        className="flex items-center text-text-dim/60 hover:text-white transition-all mb-12 group"
                    >
                        <div className="w-10 h-10 rounded-xl bg-white/5 border border-white/5 flex items-center justify-center mr-4 group-hover:bg-white/10 transition-all">
                            <ArrowLeft className="w-5 h-5 group-hover:-translate-x-1 transition-transform" />
                        </div>
                        <span className="text-[10px] font-black uppercase tracking-[0.3em]">Return to Network</span>
                    </motion.button>

                    <div className="grid grid-cols-1 lg:grid-cols-12 gap-12 items-start">

                        {/* ── LEFT COLUMN: INFO & BADGES ── */}
                        <div className="lg:col-span-4 space-y-8">

                            {/* Profile Card */}
                            <motion.div
                                initial={{ opacity: 0, y: 20 }}
                                animate={{ opacity: 1, y: 0 }}
                                className="glass rounded-[3.5rem] border-white/5 p-12 relative overflow-hidden group/card"
                            >
                                <div className="absolute top-0 right-0 p-10">
                                    <div className={`w-4 h-4 rounded-full border-4 border-background ${isOnline ? "bg-accent shadow-[0_0_15px_rgba(46,213,115,0.8)]" : "bg-white/10"}`} />
                                </div>

                                <div className="flex flex-col items-center text-center">
                                    <div className="relative mb-8">
                                        <div className="w-40 h-40 rounded-[3rem] bg-gradient-to-br from-white/10 to-white/5 border border-white/10 flex items-center justify-center overflow-hidden shadow-[0_30px_60px_-15px_rgba(0,0,0,0.5)] transition-transform duration-700 group-hover/card:scale-105">
                                            {user.avatar_url ? (
                                                <img
                                                    src={user.avatar_url}
                                                    alt={user.username}
                                                    className="w-full h-full object-cover"
                                                    onError={(e) => {
                                                        (e.target as HTMLImageElement).src = `https://api.dicebear.com/7.x/avataaars/svg?seed=${user.username}`;
                                                    }}
                                                />
                                            ) : (
                                                <div className="w-full h-full flex items-center justify-center bg-white/5">
                                                    <User className="w-16 h-16 text-white/10" />
                                                </div>
                                            )}
                                        </div>
                                    </div>

                                    <div className="space-y-4 mb-8">
                                        <div className="flex flex-col items-center gap-1">
                                            <p className="text-[10px] font-black text-text-dim/40 uppercase tracking-[0.5em] mb-2">Protocol Verified</p>
                                            <h1 className="text-4xl font-black text-white tracking-tighter italic uppercase">
                                                {user.username}
                                            </h1>
                                        </div>

                                        <p className="inline-flex items-center px-5 py-2 rounded-xl bg-white/5 border border-white/5 text-[9px] font-black uppercase tracking-[0.2em] text-text-dim/60 italic">
                                            <Calendar className="w-3.5 h-3.5 mr-2 opacity-50" />
                                            Active since {joinDate}
                                        </p>
                                    </div>

                                    {user.bio ? (
                                        <p className="text-sm text-text-dim/80 leading-relaxed max-w-xs italic mb-10">"{user.bio}"</p>
                                    ) : (
                                        <p className="text-[10px] font-black text-white/20 uppercase tracking-[0.4em] mb-10 italic">Institutional Liquidity Provider</p>
                                    )}

                                    <div className="w-full space-y-4">
                                        {isOwnProfile ? (
                                            <button
                                                onClick={() => router.push("/user/dashboard/settings")}
                                                className="w-full py-5 bg-white text-background rounded-2xl font-black uppercase tracking-[0.2em] text-[10px] hover:scale-[1.02] active:scale-[0.98] transition-all shadow-2xl shadow-white/10"
                                            >
                                                Configure Profile
                                            </button>
                                        ) : (
                                            <button
                                                className="w-full py-5 bg-primary/10 border border-primary/20 text-primary rounded-2xl font-black uppercase tracking-[0.2em] text-[10px] hover:bg-primary/20 transition-all"
                                            >
                                                Start Secure Chat
                                            </button>
                                        )}
                                    </div>
                                </div>
                            </motion.div>

                            {/* Trust Metrics */}
                            <motion.div
                                initial={{ opacity: 0, y: 20 }}
                                animate={{ opacity: 1, y: 0 }}
                                transition={{ delay: 0.1 }}
                                className="glass rounded-[2.5rem] border-white/5 p-8 space-y-6"
                            >
                                <h3 className="text-[10px] font-black text-white/20 uppercase tracking-[0.4em] mb-2 px-2">Verification Registry</h3>
                                <div className="space-y-4">
                                    <div className={`flex items-center justify-between p-5 rounded-2xl border ${user.email_verified ? "bg-accent/5 border-accent/10" : "bg-white/[0.02] border-white/5"}`}>
                                        <div className="flex items-center gap-4">
                                            <div className={`w-10 h-10 rounded-xl flex items-center justify-center ${user.email_verified ? "bg-accent/10" : "bg-white/5"}`}>
                                                <Shield className={`w-5 h-5 ${user.email_verified ? "text-accent" : "text-white/20"}`} />
                                            </div>
                                            <div className="flex flex-col gap-0.5">
                                                <span className={`text-[10px] font-black uppercase tracking-widest ${user.email_verified ? "text-white" : "text-white/30"}`}>Email Gateway</span>
                                                <span className="text-[8px] font-bold text-text-dim/40 uppercase tracking-widest">{user.email_verified ? "SECURE_CONNECTION" : "UNVERIFIED_NODE"}</span>
                                            </div>
                                        </div>
                                        {user.email_verified && <CheckCircle2 className="w-5 h-5 text-accent" />}
                                    </div>
                                </div>
                            </motion.div>
                        </div>

                        {/* ── RIGHT COLUMN: STATS & ACTIVITY ── */}
                        <div className="lg:col-span-8 space-y-12">

                            {/* Summary Totals */}
                            <div className="grid grid-cols-2 lg:grid-cols-4 gap-6">
                                {[
                                    { label: "Executions", value: stats.total_trades || 0, icon: Activity, color: "text-primary" },
                                    { label: "Completion", value: `${(stats.success_rate || 0).toFixed(1)}%`, icon: Zap, color: "text-accent" },
                                    { label: "Trust Score", value: `${(((stats.positive_feedback_count || 0) / (stats.total_trades || 1)) * 100).toFixed(0)}%`, icon: ThumbsUp, color: "text-green-500" },
                                    { label: "Ledger Volume", value: `$${((stats.total_volume_usd || 0) / 1000).toFixed(1)}k`, icon: TrendingUp, color: "text-blue-500" },
                                ].map((stat, i) => (
                                    <motion.div
                                        key={i}
                                        initial={{ opacity: 0, y: 20 }}
                                        animate={{ opacity: 1, y: 0 }}
                                        transition={{ delay: 0.2 + i * 0.05 }}
                                        className="glass rounded-[2rem] border-white/5 p-8 flex flex-col items-center justify-center text-center group hover:bg-white/[0.04] hover:border-primary/20 transition-all cursor-default"
                                    >
                                        <div className={`w-12 h-12 rounded-2xl bg-white/5 flex items-center justify-center mb-6 group-hover:scale-110 group-hover:rotate-[10deg] transition-all duration-500`}>
                                            <stat.icon className={`w-6 h-6 ${stat.color}`} />
                                        </div>
                                        <p className="text-3xl font-black text-white mb-2 italic tracking-tighter">{stat.value}</p>
                                        <p className="text-[9px] font-black text-text-dim/40 uppercase tracking-[0.3em]">{stat.label}</p>
                                    </motion.div>
                                ))}
                            </div>

                            {/* Detailed Stats & Feedback Card */}
                            <motion.div
                                initial={{ opacity: 0, y: 20 }}
                                animate={{ opacity: 1, y: 0 }}
                                transition={{ delay: 0.4 }}
                                className="glass rounded-[3.5rem] border-white/5 overflow-hidden shadow-2xl bg-white/[0.01]"
                            >
                                <div className="p-10 border-b border-white/5 bg-white/[0.02] flex flex-col sm:flex-row sm:items-center justify-between gap-6">
                                    <div className="space-y-1">
                                        <h3 className="text-2xl font-black text-white uppercase tracking-tighter italic">Network Performance</h3>
                                        <p className="text-[9px] font-black text-text-dim/40 uppercase tracking-[0.4em]">Real-time statistics from global trades</p>
                                    </div>
                                    <div className="flex items-center gap-6 px-8 py-4 bg-white/5 rounded-3xl border border-white/5">
                                        <div className="text-right">
                                            <p className="text-[8px] font-black text-text-dim/60 uppercase tracking-[0.4em] mb-1">AGGREGATE_RATING</p>
                                            <div className="flex items-center justify-end gap-3">
                                                <Star className="w-5 h-5 text-amber-500 fill-amber-500" />
                                                <span className="text-3xl font-black text-white tracking-tighter italic">
                                                    {stats.avg_rating !== undefined && stats.avg_rating !== null
                                                        ? stats.avg_rating.toFixed(2)
                                                        : (stats.total_trades && stats.total_trades > 0 ? "5.00" : "0.00")}
                                                </span>
                                            </div>
                                        </div>
                                    </div>
                                </div>

                                <div className="p-10 grid grid-cols-1 md:grid-cols-2 gap-16">
                                    {/* Speed Metrics */}
                                    <div className="space-y-10">
                                        <div className="flex items-center justify-between">
                                            <h4 className="text-[10px] font-black text-white/20 uppercase tracking-[0.4em] flex items-center gap-3 italic">
                                                <Clock className="w-4 h-4" /> LATENCY_METRICS
                                            </h4>
                                            <span className="text-[8px] font-black text-primary/40 uppercase tracking-widest">Optimized</span>
                                        </div>
                                        <div className="space-y-10">
                                            <div className="space-y-4">
                                                <div className="flex justify-between items-end">
                                                    <span className="text-[10px] font-black text-text-dim/60 uppercase tracking-[0.2em]">Payment Velocity</span>
                                                    <span className="text-lg font-black text-white italic">{formatTime(stats.avg_payment_time)}</span>
                                                </div>
                                                <div className="h-2 w-full bg-white/5 rounded-full overflow-hidden p-[2px]">
                                                    <div
                                                        className="h-full bg-gradient-to-r from-accent to-accent/40 rounded-full shadow-[0_0_15px_rgba(46,213,115,0.4)] transition-all duration-1000"
                                                        style={{ width: `${getSpeedPercentage(stats.avg_payment_time)}%` }}
                                                    />
                                                </div>
                                            </div>
                                            <div className="space-y-4">
                                                <div className="flex justify-between items-end">
                                                    <span className="text-[10px] font-black text-text-dim/60 uppercase tracking-[0.2em]">Release Velocity</span>
                                                    <span className="text-lg font-black text-white italic">{formatTime(stats.avg_release_time)}</span>
                                                </div>
                                                <div className="h-2 w-full bg-white/5 rounded-full overflow-hidden p-[2px]">
                                                    <div
                                                        className="h-full bg-gradient-to-r from-primary to-primary/40 rounded-full shadow-[0_0_15px_rgba(255,255,255,0.2)] transition-all duration-1000"
                                                        style={{ width: `${getSpeedPercentage(stats.avg_release_time)}%` }}
                                                    />
                                                </div>
                                            </div>
                                        </div>
                                    </div>

                                    {/* Feedback Breakdown */}
                                    <div className="space-y-10">
                                        <div className="flex items-center justify-between">
                                            <h4 className="text-[10px] font-black text-white/20 uppercase tracking-[0.4em] flex items-center gap-3 italic">
                                                <ThumbsUp className="w-4 h-4" /> TRUST_LEDGER
                                            </h4>
                                            <span className="text-[8px] font-black text-accent/40 uppercase tracking-widest">Audited</span>
                                        </div>
                                        <div className="space-y-6">
                                            {[
                                                { label: "Positive", value: stats.positive_feedback_count, icon: ThumbsUp, color: "text-accent", bg: "bg-accent/10", border: "border-accent/10" },
                                                { label: "Neutral", value: stats.neutral_feedback_count, icon: Star, color: "text-text-dim/60", bg: "bg-white/5", border: "border-white/5" },
                                                { label: "Negative", value: stats.negative_feedback_count, icon: ThumbsDown, color: "text-red-500", bg: "bg-red-500/10", border: "border-red-500/10" },
                                            ].map((f, idx) => (
                                                <div key={idx} className="flex items-center justify-between p-5 rounded-2xl bg-white/[0.02] border border-white/5 hover:bg-white/[0.04] transition-all">
                                                    <div className="flex items-center gap-4">
                                                        <div className={`w-10 h-10 rounded-xl ${f.bg} flex items-center justify-center border ${f.border}`}>
                                                            <f.icon className={`w-5 h-5 ${f.color}`} />
                                                        </div>
                                                        <span className="text-[10px] font-black text-white/80 uppercase tracking-widest">{f.label} Settlement</span>
                                                    </div>
                                                    <span className="text-xl font-black text-white italic">{f.value}</span>
                                                </div>
                                            ))}
                                        </div>
                                    </div>
                                </div>
                            </motion.div>

                            {/* Recent Activity / Ads */}
                            <motion.div
                                initial={{ opacity: 0, y: 20 }}
                                animate={{ opacity: 1, y: 0 }}
                                transition={{ delay: 0.5 }}
                                className="glass rounded-[3.5rem] border-white/5 p-12 bg-white/[0.01]"
                            >
                                <div className="flex items-center justify-between mb-10">
                                    <div className="space-y-1">
                                        <h3 className="text-2xl font-black text-white uppercase tracking-tighter italic">Protocol Offerings</h3>
                                        <p className="text-[9px] font-black text-text-dim/40 uppercase tracking-[0.4em]">Active liquidity channels</p>
                                    </div>
                                    <div className="flex items-center gap-3">
                                        <div className="w-2 h-2 bg-accent rounded-full animate-pulse shadow-[0_0_8px_rgba(46,213,115,0.6)]" />
                                        <span className="text-[10px] font-black text-accent uppercase tracking-widest">LIVE_NOW</span>
                                    </div>
                                </div>

                                {activeAds.length > 0 ? (
                                    <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
                                        {activeAds.map((ad, idx) => (
                                            <div
                                                key={ad.ad_id}
                                                className="p-8 rounded-[2.5rem] bg-white/[0.03] border border-white/5 hover:border-primary/40 hover:bg-white/[0.06] transition-all group cursor-pointer relative overflow-hidden"
                                                onClick={() => router.push(`/marketplace/ads/${ad.ad_id}`)}
                                            >
                                                <div className="flex justify-between items-start mb-8 relative z-10">
                                                    <div>
                                                        <div className={`px-4 py-1 rounded-xl text-[9px] font-black uppercase tracking-[0.3em] mb-4 inline-block shadow-lg ${ad.type === 'buy' ? 'bg-primary/10 text-primary border border-primary/20' : 'bg-accent/10 text-accent border border-accent/20'}`}>
                                                            {ad.type} {ad.crypto_symbol}
                                                        </div>
                                                        <h4 className="text-3xl font-black text-white italic tracking-tighter">
                                                            {ad.price.toLocaleString()} <span className="text-[10px] text-text-dim/40 non-italic ml-2 uppercase tracking-widest">{ad.fiat_symbol}</span>
                                                        </h4>
                                                    </div>
                                                    <div className="text-right">
                                                        <p className="text-[8px] font-black text-text-dim/40 uppercase tracking-[0.4em] mb-1">BOUNDARIES</p>
                                                        <p className="text-[11px] font-black text-white tracking-widest italic">{ad.min_amount} - {ad.max_amount}</p>
                                                    </div>
                                                </div>
                                                <div className="flex items-center gap-3 relative z-10">
                                                    {ad.payment_methods.slice(0, 2).map((pm, i) => (
                                                        <span key={i} className="text-[9px] font-black uppercase tracking-widest text-white/40 bg-white/5 border border-white/5 px-4 py-2 rounded-xl">
                                                            {pm}
                                                        </span>
                                                    ))}
                                                    {ad.payment_methods.length > 2 && (
                                                        <span className="text-[9px] font-black text-white/20 uppercase tracking-widest ml-1">+{ad.payment_methods.length - 2} Channels</span>
                                                    )}
                                                </div>
                                                <div className="absolute top-0 right-0 p-8 opacity-0 group-hover:opacity-10 transition-opacity">
                                                    <TrendingUp className="w-16 h-16 text-primary" />
                                                </div>
                                            </div>
                                        ))}
                                    </div>
                                ) : (
                                    <div className="text-center py-24 border-2 border-dashed border-white/5 rounded-[3rem] bg-white/[0.01]">
                                        <TrendingUp className="w-16 h-16 text-white/5 mx-auto mb-6" />
                                        <p className="text-text-dim/60 font-black uppercase tracking-[0.4em] text-sm italic">No active public liquidity channels.</p>
                                        <p className="text-[10px] uppercase font-black text-white/10 tracking-[0.5em] mt-3 italic">Integrator operating in stealth mode</p>
                                    </div>
                                )}
                            </motion.div>
                        </div>
                    </div>
                </div>
            </div>

            <Footer />
        </main>
    );
};

export default PublicProfilePage;
