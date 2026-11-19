"use client";

import React from "react";
import {
    Search,
    Bell,
    Menu,
    ChevronDown,
    User as UserIcon,
    Activity
} from "lucide-react";
import { motion } from "framer-motion";
import { useAuth } from "@/context/AuthContext";
import { useNotifications } from "@/context/NotificationContext";
import Link from "next/link";

interface DashboardHeaderProps {
    title: string;
    onMenuClick?: () => void;
}

const DashboardHeader = ({ title, onMenuClick }: DashboardHeaderProps) => {
    const { user } = useAuth();
    const { unreadCount, isConnected } = useNotifications();

    return (
        <header className="sticky top-0 z-30 flex items-center justify-between px-6 md:px-10 py-6 bg-background/60 backdrop-blur-2xl border-b border-white/5 mx-6 mt-6 rounded-[2.5rem]">
            <div className="flex items-center space-x-6">
                <button
                    onClick={onMenuClick}
                    className="p-3 md:hidden hover:bg-white/5 rounded-2xl transition-all border border-white/5 active:scale-90"
                >
                    <Menu className="w-5 h-5 text-white" />
                </button>
                <div className="hidden md:block">
                    <h1 className="text-2xl font-black text-white tracking-tighter italic uppercase">{title}</h1>
                </div>
            </div>

            <div className="flex items-center space-x-4 md:space-x-8">
                {/* Search Bar - Desktop */}
                <div className="hidden lg:flex items-center relative group">
                    <Search className="absolute left-5 w-4 h-4 text-text-dim/40 group-focus-within:text-primary transition-all duration-300" />
                    <input
                        type="text"
                        placeholder="Search ecosystem..."
                        className="bg-white/5 border border-white/5 py-4 pl-14 pr-8 rounded-[1.5rem] text-xs w-72 lg:w-96 outline-none focus:border-primary/50 focus:bg-white/[0.08] transition-all font-bold placeholder:text-text-dim/20 tracking-wider"
                    />
                </div>

                {/* Global Stats */}
                <div className="hidden sm:flex items-center space-x-6 px-6 py-3 bg-white/[0.03] rounded-2xl border border-white/5 hover:bg-white/[0.05] transition-colors cursor-pointer group">
                    <div className="flex items-center space-x-3">
                        <div className={`w-2 h-2 rounded-full shadow-[0_0_8px_rgba(var(--accent-rgb),0.5)] ${isConnected ? 'bg-accent animate-pulse' : 'bg-primary'}`} />
                        <span className="text-[10px] font-black text-white/60 tracking-[0.2em] uppercase group-hover:text-white transition-colors">
                            {isConnected ? 'Escrow Online' : 'Synchronizing...'}
                        </span>
                    </div>
                </div>

                {/* Profile Dropdown */}
                <div className="flex items-center gap-4 pl-4 border-l border-white/5">
                    <div className="flex flex-col items-end hidden sm:flex">
                        <span className="text-xs font-black text-white tracking-tight italic">{user?.name || "Anonymous User"}</span>
                        <span className="text-[9px] font-black text-accent uppercase tracking-widest">{user?.role || "Protocol Participant"}</span>
                    </div>
                    <div className="relative cursor-pointer group">
                        <div className="w-12 h-12 rounded-2xl bg-gradient-to-br from-primary/20 via-secondary/10 to-transparent border border-white/10 flex items-center justify-center overflow-hidden transition-all duration-500 group-hover:scale-105 shadow-xl shadow-primary/5">
                            {user?.avatarUrl ? (
                                <img src={user.avatarUrl} alt={user.name} className="w-full h-full object-cover" />
                            ) : (
                                <UserIcon className="w-6 h-6 text-white/20 group-hover:text-white/60 transition-colors" />
                            )}
                        </div>
                        <div className={`absolute -top-1 -right-1 w-4 h-4 ${user?.isOnline ? 'bg-accent' : 'bg-text-dim/40'} border-[3px] border-background rounded-full shadow-lg shadow-black/40`} />
                    </div>
                </div>
            </div>
        </header>
    );
};

export default DashboardHeader;
