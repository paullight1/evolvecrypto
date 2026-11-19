"use client";

import React, { useState } from "react";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { Wallet, Mail, LogOut, Menu, X, User } from "lucide-react";
import { motion, AnimatePresence } from "framer-motion";
import { useAuth } from "@/context/AuthContext";
import { authService } from "@/services/authService";
import NotificationBell from "./NotificationBell";

const Navbar = () => {
    const { user, logout, isLoading } = useAuth();
    const router = useRouter();
    const [isMobileMenuOpen, setIsMobileMenuOpen] = useState(false);
    const [isResending, setIsResending] = useState(false);

    const handleLogout = async () => {
        await logout();
        router.push("/login");
    };

    const handleResendVerification = async () => {
        if (!user) {
            return;
        }

        setIsResending(true);
        try {
            await authService.requestEmailVerification(user.id);
        } catch (error) {
            console.error("Failed to resend:", error);
        } finally {
            setIsResending(false);
        }
    };

    return (
        <nav className="fixed top-0 w-full bg-background/60 backdrop-blur-2xl border-b border-white/5 z-50 py-4">
            <div className="container mx-auto px-6 md:px-10">
                <div className="flex items-center justify-between h-20">
                    {/* Logo */}
                    <Link href="/" className="flex items-center space-x-4 group">
                        <div className="w-12 h-12 bg-gradient-to-br from-primary to-secondary rounded-2xl flex items-center justify-center shadow-2xl shadow-primary/20 group-hover:rotate-[15deg] transition-all duration-700">
                            <Wallet className="w-6 h-6 text-white" />
                        </div>
                        <div className="flex flex-col">
                            <span className="text-2xl font-black italic uppercase tracking-tighter text-white leading-none">
                                CRYP<span className="text-primary truncate">LIO</span>
                            </span>
                            <span className="text-[8px] font-black tracking-[0.5em] text-white/30 uppercase mt-1">Institutional Grade</span>
                        </div>
                    </Link>

                    {/* Desktop Navigation */}
                    <div className="hidden md:flex items-center space-x-12">
                        {["Marketplace", "Support"].map((item) => (
                            <Link
                                key={item}
                                href={`/${item.toLowerCase()}`}
                                className="text-[11px] font-black text-text-dim/60 uppercase tracking-[0.3em] hover:text-white transition-all relative group py-2"
                            >
                                {item}
                                <span className="absolute bottom-0 left-0 w-0 h-[2px] bg-primary transition-all duration-500 group-hover:w-full" />
                            </Link>
                        ))}

                        <div className="h-6 w-[1px] bg-white/10" />

                        {user ? (
                            <div className="flex items-center space-x-8">
                                <NotificationBell />
                                <Link
                                    href="/user/dashboard"
                                    className="relative flex items-center px-8 py-3 bg-white text-background rounded-2xl text-[10px] font-black uppercase tracking-widest hover:scale-105 active:scale-95 transition-all shadow-xl shadow-white/5"
                                >
                                    Access Protocol
                                </Link>
                                <button
                                    onClick={handleLogout}
                                    disabled={isLoading}
                                    className="p-3 text-red-500/60 hover:text-red-500 hover:bg-red-500/5 rounded-2xl transition-all border border-transparent hover:border-red-500/20"
                                    title="Logout"
                                >
                                    <LogOut className="w-5 h-5" />
                                </button>
                            </div>
                        ) : (
                            <div className="flex items-center space-x-8">
                                <Link href="/login" className="text-[11px] font-black text-white/40 hover:text-white uppercase tracking-[0.3em] transition-all">
                                    Login
                                </Link>
                                <Link href="/register" className="relative p-[1px] group">
                                    <div className="absolute inset-0 bg-gradient-to-r from-primary to-secondary rounded-2xl opacity-50 blur-[2px] group-hover:opacity-100 transition-opacity" />
                                    <div className="relative bg-background px-8 py-3.5 rounded-2xl text-[10px] font-black uppercase tracking-widest text-white border border-white/10 group-hover:border-transparent transition-all">
                                        Initialize Account
                                    </div>
                                </Link>
                            </div>
                        )}
                    </div>

                    {/* Mobile menu button */}
                    <button
                        onClick={() => setIsMobileMenuOpen(!isMobileMenuOpen)}
                        className="md:hidden p-2 text-text-dim hover:text-white"
                    >
                        {isMobileMenuOpen ? <X className="w-6 h-6" /> : <Menu className="w-6 h-6" />}
                    </button>
                </div>
            </div>

            {/* Mobile Menu */}
            <AnimatePresence>
                {isMobileMenuOpen && (
                    <motion.div
                        initial={{ opacity: 0, height: 0 }}
                        animate={{ opacity: 1, height: "auto" }}
                        exit={{ opacity: 0, height: 0 }}
                        className="md:hidden bg-surface border-t border-white/5 overflow-hidden"
                    >
                        <div className="px-4 py-6 space-y-4">
                            <Link href="/marketplace" className="block text-sm font-bold text-text-dim hover:text-white">
                                Marketplace
                            </Link>
                            {/* <Link href="/swap" className="block text-sm font-bold text-text-dim hover:text-white">
                                Swap
                            </Link> */}
                            <Link href="/support" className="block text-sm font-bold text-text-dim hover:text-white">
                                Support
                            </Link>

                            {user ? (
                                <>
                                    <div className="pt-4 border-t border-white/5">
                                        <p className="text-xs font-black text-text-dim uppercase tracking-widest mb-3">
                                            Account
                                        </p>
                                        <Link href="/user/dashboard" className="block py-2 text-sm font-medium">
                                            Dashboard
                                        </Link>

                                        {/* Email verification prompt */}
                                        {!user.emailVerified && (
                                            <button
                                                onClick={handleResendVerification}
                                                disabled={isResending}
                                                className="flex items-center space-x-2 py-2 text-sm text-yellow-500"
                                            >
                                                <Mail className="w-4 h-4" />
                                                <span>{isResending ? "Sending..." : "Resend verification email"}</span>
                                            </button>
                                        )}

                                        <button
                                            onClick={handleLogout}
                                            className="flex items-center space-x-2 py-2 text-sm text-red-400"
                                        >
                                            <LogOut className="w-4 h-4" />
                                            <span>Logout</span>
                                        </button>
                                    </div>
                                </>
                            ) : (
                                <div className="pt-4 border-t border-white/5 space-y-3">
                                    <Link href="/login" className="block w-full text-center py-3 border border-white/10 rounded-xl text-sm font-bold">
                                        Login
                                    </Link>
                                    <Link href="/register" className="block w-full text-center bg-white text-background py-3 rounded-xl text-sm font-black">
                                        Sign Up
                                    </Link>
                                </div>
                            )}
                        </div>
                    </motion.div>
                )}
            </AnimatePresence>
        </nav>
    );
};

export default Navbar;
