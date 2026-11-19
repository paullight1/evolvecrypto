"use client";

import React, { useState } from "react";
import Link from "next/link";
import { usePathname } from "next/navigation";
import { useAuth } from "@/context/AuthContext";
import {
    Wallet,
    LayoutDashboard,
    Settings,
    Shield,
    ShieldAlert,
    LogOut,
    CreditCard,
    Users,
    Store,
    X,
    Coins,
    DollarSign,
    ArrowUpRight,
    Megaphone,
    History
} from "lucide-react";
import { motion } from "framer-motion";
import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";

function cn(...inputs: ClassValue[]) {
    return twMerge(clsx(inputs));
}

const DISMISSED_STATUS_CARDS_KEY = "cryplio_dismissed_status_cards";

interface SidebarItem {
    name: string;
    href: string;
    icon: React.ElementType;
}

interface SidebarProps {
    role: "user" | "admin";
    isMobile?: boolean;
}

const Sidebar = ({ role, isMobile }: SidebarProps) => {
    const pathname = usePathname();
    const { user, logout, isLoading } = useAuth();
    const [dismissedStatusCards, setDismissedStatusCards] = useState<string[]>(() => {
        if (typeof window === "undefined") {
            return [];
        }

        try {
            return JSON.parse(localStorage.getItem(DISMISSED_STATUS_CARDS_KEY) || "[]");
        } catch {
            return [];
        }
    });

    const showStatusCard = Boolean(user && !dismissedStatusCards.includes(user.id) && !user.emailVerified);

    const dismissStatusCard = () => {
        if (!user) {
            return;
        }

        const nextDismissedCards = Array.from(new Set([...dismissedStatusCards, user.id]));
        setDismissedStatusCards(nextDismissedCards);
        localStorage.setItem(DISMISSED_STATUS_CARDS_KEY, JSON.stringify(nextDismissedCards));
    };

    const navigation: Record<string, SidebarItem[]> = {
        user: [
            { name: "Overview", href: "/user/dashboard", icon: LayoutDashboard },
            { name: "Marketplace", href: "/marketplace", icon: Store },
            { name: "My Trades", href: "/user/dashboard/trades", icon: History },
            { name: "My Postings", href: "/user/dashboard/postings", icon: Megaphone },
            { name: "Wallet", href: "/user/dashboard/wallet", icon: CreditCard },
            { name: "Settings", href: "/user/dashboard/settings", icon: Settings },
        ],
        admin: [
            { name: "Overview", href: "/admin/dashboard", icon: LayoutDashboard },
            { name: "User Management", href: "/admin/dashboard/users", icon: Users },
            { name: "Withdrawals", href: "/admin/dashboard/withdrawals", icon: ArrowUpRight },
            { name: "Disputes", href: "/admin/dashboard/disputes", icon: ShieldAlert },
            { name: "Payment Methods", href: "/admin/dashboard/payment-methods", icon: CreditCard },
            { name: "Crypto Assets", href: "/admin/dashboard/crypto-assets", icon: Coins },
            { name: "Fiat Currencies", href: "/admin/dashboard/fiat-currencies", icon: DollarSign },
        ],
    };

    const navItems = navigation[role] || [];

    return (
        <aside className={cn(
            "w-72 flex flex-col bg-surface overflow-y-auto scrollbar-hide",
            isMobile
                ? "h-full border-0"
                : "hidden md:flex border-r border-border h-screen fixed top-0 left-0 z-20"
        )}>
            <div className="p-8">
                <Link href="/" className="flex items-center space-x-3 group">
                    <div className="w-10 h-10 bg-primary rounded-xl flex items-center justify-center shadow-xl shadow-primary/20 group-hover:rotate-12 transition-transform duration-500">
                        <Wallet className="text-background w-6 h-6" />
                    </div>
                    <span className="text-2xl font-black italic uppercase tracking-tighter text-white">
                        CRYP<span className="text-primary truncate">LIO</span>
                    </span>
                </Link>
            </div>

            <nav className="flex-1 px-6 space-y-2 mt-4">
                <div className="px-4 py-2 mb-2">
                    <span className="text-[10px] font-black text-text-dim/40 uppercase tracking-[0.3em]">
                        Standard Gateway
                    </span>
                </div>
                {navItems.map((item) => {
                    const isActive = pathname === item.href;
                    return (
                        <Link
                            key={item.name}
                            href={item.href}
                            className={cn(
                                "relative flex items-center group px-5 py-4 rounded-2xl transition-all duration-500 overflow-hidden",
                                isActive
                                    ? "text-white shadow-2xl shadow-primary/20"
                                    : "text-text-dim hover:text-white"
                            )}
                        >
                            {/* Active Background Pill */}
                            {isActive && (
                                <motion.div
                                    layoutId="sidebar-active"
                                    className="absolute inset-0 bg-gradient-to-r from-primary/20 via-primary/10 to-transparent border-l-2 border-primary"
                                    transition={{ type: "spring", bounce: 0.2, duration: 0.6 }}
                                />
                            )}

                            <item.icon className={cn(
                                "w-5 h-5 relative z-10 transition-all duration-500",
                                isActive ? "text-primary scale-110 mr-4" : "text-text-dim group-hover:text-white mr-4 group-hover:scale-110"
                            )} />

                            <span className={cn(
                                "relative z-10 font-black text-[11px] uppercase tracking-widest transition-all duration-500",
                                isActive ? "translate-x-0" : "group-hover:translate-x-1"
                            )}>
                                {item.name}
                            </span>

                            {isActive && (
                                <motion.div
                                    initial={{ opacity: 0, x: -10 }}
                                    animate={{ opacity: 1, x: 0 }}
                                    className="ml-auto relative z-10"
                                >
                                    <div className="w-1.5 h-1.5 rounded-full bg-primary shadow-[0_0_10px_rgba(var(--primary-rgb),0.8)]" />
                                </motion.div>
                            )}
                        </Link>
                    );
                })}
            </nav>

            <div className="p-4 mt-auto">
                {showStatusCard && user && (
                    <div className="relative p-4 rounded-2xl bg-surface-light border border-white/5 mb-4">
                        <button
                            type="button"
                            onClick={dismissStatusCard}
                            className="absolute right-3 top-3 p-1.5 rounded-lg text-text-dim hover:text-white hover:bg-white/5 transition-colors"
                            aria-label="Dismiss account status"
                        >
                            <X className="w-3.5 h-3.5" />
                        </button>
                        <div className="flex items-center space-x-3 mb-3">
                            <div className={cn(
                                "w-8 h-8 rounded-lg flex items-center justify-center",
                                user.emailVerified ? "bg-accent/20" : "bg-yellow-500/15"
                            )}>
                                <Shield className={cn(
                                    "w-4 h-4",
                                    user.emailVerified ? "text-accent" : "text-yellow-500"
                                )} />
                            </div>
                            <span className="text-xs font-bold text-white tracking-tight">
                                {user.emailVerified ? "Email Verified" : "Email Not Verified"}
                            </span>
                        </div>
                        <p className="text-[10px] text-text-dim font-medium leading-relaxed mb-3">
                            {user.emailVerified
                                ? "Your email is verified and account is secured."
                                : "Please verify your email to secure your account."}
                        </p>
                        <div className="w-full bg-white/5 h-1 rounded-full overflow-hidden">
                            <div className={cn(
                                "h-full rounded-full",
                                user.emailVerified ? "w-full bg-accent" : "w-1/3 bg-yellow-500"
                            )} />
                        </div>
                    </div>
                )}

                <button
                    type="button"
                    onClick={() => void logout()}
                    disabled={isLoading}
                    className="flex items-center w-full px-4 py-3 text-text-dim hover:text-white hover:bg-white/5 rounded-xl transition-all group border border-transparent disabled:opacity-60 disabled:cursor-not-allowed"
                >
                    <LogOut className="w-5 h-5 mr-3 group-hover:text-white transition-colors" />
                    <span className="font-bold text-sm tracking-tight text-white/60 group-hover:text-white">
                        {isLoading ? "Logging out..." : "Logout"}
                    </span>
                </button>
            </div>
        </aside>
    );
};

export default Sidebar;
