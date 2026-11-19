"use client";

import React, { useEffect, useMemo, useState } from "react";
import { motion } from "framer-motion";
import DashboardLayout from "@/components/dashboard/DashboardLayout";
import {
    ArrowUpRight,
    ArrowDownLeft,
    History,
    Wallet as WalletIcon,
    RefreshCw,
    Copy,
    ExternalLink,
    AlertTriangle,
    Shield,
    Loader2,
    X,
    CheckCircle2,
    QrCode,
    Clock
} from "lucide-react";
import { WalletService, WalletBalance, WalletTransaction } from "@/services/walletService";
import { toast } from "sonner";

const UserWallet = () => {
    const [wallets, setWallets] = useState<WalletBalance[]>([]);
    const [transactions, setTransactions] = useState<WalletTransaction[]>([]);
    const [loading, setLoading] = useState(true);
    const [error, setError] = useState<string | null>(null);

    // Modal states
    const [showDepositModal, setShowDepositModal] = useState(false);
    const [showWithdrawModal, setShowWithdrawModal] = useState(false);
    const [selectedWallet, setSelectedWallet] = useState<WalletBalance | null>(null);
    const [depositAddress, setDepositAddress] = useState<string>("");
    const [withdrawForm, setWithdrawForm] = useState({
        amount: "",
        address: "",
        twoFACode: ""
    });
    const [isSubmitting, setIsSubmitting] = useState(false);

    const loadWalletData = async () => {
        setLoading(true);
        setError(null);
        try {
            const [walletData, txData] = await Promise.all([
                WalletService.getBalances(),
                WalletService.getTransactions({ limit: 10, offset: 0 }),
            ]);
            setWallets(walletData);
            setTransactions(txData.transactions || txData || []);
        } catch (err) {
            setError(err instanceof Error ? err.message : "Failed to load wallet data");
        } finally {
            setLoading(false);
        }
    };

    useEffect(() => {
        void loadWalletData();
    }, []);

    const totalBalance = useMemo(
        () => wallets.reduce((sum, wallet) => sum + Number(wallet.balance || 0), 0),
        [wallets]
    );

    const handleDeposit = async (wallet: WalletBalance) => {
        setSelectedWallet(wallet);
        setShowDepositModal(true);

        try {
            const cryptoSymbol = wallet.crypto_symbol || "USDT";
            const addressData = await WalletService.getDepositAddress(cryptoSymbol);
            setDepositAddress(addressData.address);
        } catch (error: any) {
            toast.error("Failed to get deposit address");
            setShowDepositModal(false);
        }
    };

    const handleWithdraw = async (wallet: WalletBalance) => {
        if (Number(wallet.balance) <= 0) {
            toast.error("Insufficient balance");
            return;
        }

        setSelectedWallet(wallet);
        setShowWithdrawModal(true);
    };

    const handleWithdrawSubmit = async () => {
        if (!withdrawForm.amount || !withdrawForm.address || !withdrawForm.twoFACode) {
            toast.error("Please fill all fields");
            return;
        }

        if (Number(withdrawForm.amount) > Number(selectedWallet?.balance)) {
            toast.error("Insufficient balance");
            return;
        }

        setIsSubmitting(true);
        try {
            await WalletService.withdraw({
                crypto: selectedWallet?.crypto_symbol || "USDT",
                amount: Number(withdrawForm.amount),
                address: withdrawForm.address
            });

            toast.success("Withdrawal request submitted successfully");
            setShowWithdrawModal(false);
            setWithdrawForm({ amount: "", address: "", twoFACode: "" });
            loadWalletData();
        } catch (error: any) {
            toast.error(error.message || "Failed to submit withdrawal");
        } finally {
            setIsSubmitting(false);
        }
    };

    const copyToClipboard = (text: string) => {
        navigator.clipboard.writeText(text);
        toast.success("Copied to clipboard");
    };

    return (
        <DashboardLayout title="Universal Wallet" role="user">
            <div className="space-y-10 pb-20">
                {/* Total Balance Card */}
                <div className="relative overflow-hidden group">
                    <div className="absolute inset-0 bg-gradient-to-br from-primary/10 via-secondary/10 to-transparent opacity-50 group-hover:opacity-100 transition-opacity duration-1000" />
                    <div className="glass relative rounded-[3rem] border border-white/5 p-12 flex flex-col md:flex-row md:items-center justify-between gap-8 bg-white/[0.02]">
                        <div className="flex items-center gap-8">
                            <div className="w-24 h-24 rounded-[2rem] bg-gradient-to-br from-primary to-secondary flex items-center justify-center shadow-2xl shadow-primary/20 rotate-3 group-hover:rotate-0 transition-transform duration-500">
                                <WalletIcon className="text-white w-10 h-10" />
                            </div>
                            <div>
                                <p className="text-[10px] font-black text-text-dim/40 uppercase tracking-[0.4em] mb-3">Aggregated Governance Balance</p>
                                <div className="flex items-baseline gap-3">
                                    <h2 className="text-6xl font-black text-white italic tracking-tighter leading-none">{totalBalance.toFixed(2)}</h2>
                                    <span className="text-xl font-black text-text-dim uppercase tracking-widest italic opacity-40">USDT</span>
                                </div>
                                <div className="flex items-center gap-3 mt-4">
                                    <div className="px-3 py-1 bg-accent/10 border border-accent/20 rounded-full flex items-center gap-2">
                                        <div className="w-1.5 h-1.5 bg-accent rounded-full animate-pulse" />
                                        <span className="text-[9px] font-black text-accent uppercase tracking-widest">Protocol Sync: 100%</span>
                                    </div>
                                    <span className="text-[9px] font-black text-text-dim/40 uppercase tracking-widest">Global Asset ID: #WLT-8392-CZ</span>
                                </div>
                            </div>
                        </div>
                        <button
                            onClick={() => void loadWalletData()}
                            className="flex items-center gap-3 px-8 py-4 bg-white/5 border border-white/10 rounded-2xl text-[10px] font-black uppercase tracking-widest hover:bg-white/10 hover:border-white/20 transition-all active:scale-95 text-white/60"
                        >
                            <RefreshCw className={`w-4 h-4 ${loading ? 'animate-spin text-primary' : ''}`} />
                            Sync Data
                        </button>
                    </div>
                </div>

                {error && (
                    <motion.div
                        initial={{ opacity: 0, y: -20 }}
                        animate={{ opacity: 1, y: 0 }}
                        className="bg-red-500/10 border border-red-500/20 rounded-2xl p-6 text-xs font-black text-red-500 uppercase tracking-widest flex items-center gap-3"
                    >
                        <AlertTriangle className="w-5 h-5" />
                        {error}
                    </motion.div>
                )}

                <div className="grid grid-cols-1 lg:grid-cols-2 gap-10">
                    {/* Wallet Balances */}
                    <div className="space-y-6">
                        <div className="flex items-center justify-between px-4">
                            <h3 className="text-xl font-black text-white uppercase tracking-tighter italic">Vault <span className="gradient-text">Inventory</span></h3>
                            <span className="text-[10px] font-black text-text-dim/40 uppercase tracking-widest">{wallets.length} Segregated Wallets</span>
                        </div>
                        <div className="grid grid-cols-1 gap-6">
                            {loading && <div className="p-20 text-center animate-pulse"><Loader2 className="w-10 h-10 text-primary animate-spin mx-auto" /></div>}
                            {!loading && wallets.length === 0 && (
                                <div className="p-16 rounded-[2.5rem] bg-white/[0.02] border border-white/5 border-dashed text-center space-y-4">
                                    <Shield className="w-12 h-12 text-white/5 mx-auto" />
                                    <p className="text-xs font-black text-text-dim uppercase tracking-widest">Protocol Creation Pending</p>
                                </div>
                            )}
                            {wallets.map((wallet) => (
                                <div key={wallet.wallet_id} className="glass group/item relative overflow-hidden rounded-[2.5rem] border border-white/5 p-8 bg-white/[0.02] hover:bg-white/[0.04] transition-all duration-500">
                                    <div className="flex items-center justify-between mb-8">
                                        <div className="flex items-center gap-4">
                                            <div className="w-12 h-12 rounded-2xl bg-white/5 border border-white/10 flex items-center justify-center font-black italic text-primary">T</div>
                                            <div>
                                                <h4 className="text-sm font-black text-white uppercase tracking-wider">{wallet.crypto_symbol || "USDT"} Segregation</h4>
                                                <p className="text-[9px] font-black text-text-dim/40 uppercase tracking-widest mt-0.5">TRC20 Network Interface</p>
                                            </div>
                                        </div>
                                        <div className={`px-3 py-1 rounded-full text-[9px] font-black uppercase tracking-widest border ${wallet.is_active ? "bg-accent/10 text-accent border-accent/20" : "bg-red-500/10 text-red-500 border-red-500/20"}`}>
                                            {wallet.is_active ? "Online" : "Offline"}
                                        </div>
                                    </div>

                                    <div className="flex items-end justify-between mb-8">
                                        <div className="space-y-1">
                                            <p className="text-[9px] font-black text-text-dim/40 uppercase tracking-widest">Settlement Balance</p>
                                            <h3 className="text-4xl font-black text-white italic tracking-tighter">{Number(wallet.balance).toFixed(4)}</h3>
                                        </div>
                                        {Number(wallet.locked_balance) > 0 && (
                                            <div className="text-right space-y-1">
                                                <p className="text-[9px] font-black text-orange-500/60 uppercase tracking-widest">Escrow Lock</p>
                                                <h4 className="text-lg font-black text-orange-500 italic tracking-tighter">{Number(wallet.locked_balance).toFixed(4)}</h4>
                                            </div>
                                        )}
                                    </div>

                                    <div className="flex gap-3 mt-4">
                                        <button
                                            onClick={() => handleDeposit(wallet)}
                                            className="flex-1 py-4 bg-white text-background rounded-2xl text-[10px] font-black uppercase tracking-widest hover:scale-[1.02] active:scale-[0.98] transition-all flex items-center justify-center gap-2"
                                        >
                                            <ArrowDownLeft className="w-4 h-4" />
                                            Liquidity Ingress
                                        </button>
                                        <button
                                            onClick={() => handleWithdraw(wallet)}
                                            disabled={Number(wallet.balance) <= 0}
                                            className="flex-1 py-4 bg-white/5 border border-white/10 text-white rounded-2xl text-[10px] font-black uppercase tracking-widest hover:bg-white/10 active:scale-[0.98] transition-all disabled:opacity-20 flex items-center justify-center gap-2"
                                        >
                                            <ArrowUpRight className="w-4 h-4" />
                                            Asset Egress
                                        </button>
                                    </div>
                                </div>
                            ))}
                        </div>
                    </div>

                    {/* Transaction History */}
                    <div className="space-y-6">
                        <div className="flex items-center justify-between px-4">
                            <h3 className="text-xl font-black text-white uppercase tracking-tighter italic">Ledger <span className="gradient-text">Stream</span></h3>
                            <span className="text-[10px] font-black text-text-dim/40 uppercase tracking-widest">Last 10 Records</span>
                        </div>
                        <div className="space-y-3">
                            {transactions.map((tx) => (
                                <div key={tx.tx_id || (tx as any).id} className="group flex items-center justify-between p-6 rounded-3xl bg-white/[0.02] border border-white/5 hover:bg-white/[0.04] transition-all duration-300">
                                    <div className="flex items-center gap-6">
                                        <div className={`w-14 h-14 rounded-2xl flex items-center justify-center shadow-lg transition-transform group-hover:scale-110 ${tx.type === "withdrawal" ? "bg-primary/20 text-primary" : "bg-accent/20 text-accent"
                                            }`}>
                                            {tx.type === "withdrawal" ? <ArrowUpRight className="w-6 h-6" /> : <ArrowDownLeft className="w-6 h-6" />}
                                        </div>
                                        <div>
                                            <div className="flex items-center gap-3">
                                                <p className="text-xs font-black text-white uppercase tracking-[0.1em]">{tx.type}</p>
                                                <span className={`px-2 py-0.5 rounded text-[8px] font-black uppercase tracking-tighter border ${tx.status === 'completed' ? 'bg-accent/10 text-accent border-accent/20' : 'bg-primary/10 text-primary border-primary/20'}`}>
                                                    {tx.status}
                                                </span>
                                            </div>
                                            <p className="text-[10px] font-bold text-text-dim/40 uppercase tracking-widest mt-1.5 flex items-center gap-2">
                                                <Clock className="w-3 h-3" />
                                                {new Date(tx.created_at).toLocaleString([], { dateStyle: 'medium', timeStyle: 'short' })}
                                            </p>
                                        </div>
                                    </div>
                                    <div className="text-right">
                                        <p className="text-lg font-black text-white italic tracking-tight">{tx.amount.toString()} <span className="text-[10px] non-italic text-text-dim font-black ml-1">USDT</span></p>
                                        <p className="text-[9px] font-black text-text-dim/30 uppercase tracking-[0.2em] mt-1">Settled on TRC20</p>
                                    </div>
                                </div>
                            ))}
                        </div>
                    </div>
                </div>
            </div>

            {/* Deposit Modal */}
            {showDepositModal && selectedWallet && (
                <motion.div
                    initial={{ opacity: 0 }}
                    animate={{ opacity: 1 }}
                    className="fixed inset-0 bg-black/50 backdrop-blur-sm z-50 flex items-center justify-center p-4"
                    onClick={() => setShowDepositModal(false)}
                >
                    <motion.div
                        initial={{ scale: 0.95, opacity: 0 }}
                        animate={{ scale: 1, opacity: 1 }}
                        className="bg-surface border border-white/10 rounded-2xl p-6 max-w-md w-full"
                        onClick={(e) => e.stopPropagation()}
                    >
                        <div className="flex items-center justify-between mb-6">
                            <h3 className="text-xl font-black text-white">Deposit {selectedWallet.crypto_symbol || "USDT"}</h3>
                            <button
                                onClick={() => setShowDepositModal(false)}
                                className="text-text-dim hover:text-white transition-colors"
                            >
                                <X className="w-5 h-5" />
                            </button>
                        </div>

                        <div className="space-y-4">
                            <div className="bg-white/5 border border-white/10 rounded-xl p-4">
                                <div className="flex items-center justify-between mb-2">
                                    <span className="text-sm font-bold text-white">Your Deposit Address</span>
                                    <button
                                        onClick={() => copyToClipboard(depositAddress)}
                                        className="p-2 text-primary hover:bg-primary/20 rounded-lg transition-colors"
                                    >
                                        <Copy className="w-4 h-4" />
                                    </button>
                                </div>
                                <p className="text-xs text-text-dim font-mono break-all">{depositAddress}</p>
                            </div>

                            <div className="bg-yellow-500/10 border border-yellow-500/20 rounded-xl p-4">
                                <div className="flex items-start gap-3">
                                    <AlertTriangle className="w-5 h-5 text-yellow-500 flex-shrink-0 mt-0.5" />
                                    <div>
                                        <p className="text-sm font-bold text-yellow-500 mb-1">Important Notice</p>
                                        <ul className="text-xs text-yellow-500/80 space-y-1">
                                            <li>• Only send {selectedWallet.crypto_symbol || "USDT"} to this address</li>
                                            <li>• Minimum deposit: 10 USDT</li>
                                            <li>• Deposits require 12 network confirmations</li>
                                            <li>• Sending other tokens may result in permanent loss</li>
                                        </ul>
                                    </div>
                                </div>
                            </div>

                            <div className="flex items-center gap-2 text-xs text-text-dim">
                                <Shield className="w-4 h-4 text-accent" />
                                <span>All deposits are protected by multi-signature escrow</span>
                            </div>
                        </div>
                    </motion.div>
                </motion.div>
            )}

            {/* Withdrawal Modal */}
            {showWithdrawModal && selectedWallet && (
                <motion.div
                    initial={{ opacity: 0 }}
                    animate={{ opacity: 1 }}
                    className="fixed inset-0 bg-black/50 backdrop-blur-sm z-50 flex items-center justify-center p-4"
                    onClick={() => setShowWithdrawModal(false)}
                >
                    <motion.div
                        initial={{ scale: 0.95, opacity: 0 }}
                        animate={{ scale: 1, opacity: 1 }}
                        className="bg-surface border border-white/10 rounded-2xl p-6 max-w-md w-full"
                        onClick={(e) => e.stopPropagation()}
                    >
                        <div className="flex items-center justify-between mb-6">
                            <h3 className="text-xl font-black text-white">Withdraw {selectedWallet.crypto_symbol || "USDT"}</h3>
                            <button
                                onClick={() => setShowWithdrawModal(false)}
                                className="text-text-dim hover:text-white transition-colors"
                            >
                                <X className="w-5 h-5" />
                            </button>
                        </div>

                        <div className="space-y-4">
                            <div>
                                <label className="block text-sm font-bold text-white mb-2">Amount</label>
                                <div className="relative">
                                    <input
                                        type="number"
                                        value={withdrawForm.amount}
                                        onChange={(e) => setWithdrawForm({ ...withdrawForm, amount: e.target.value })}
                                        placeholder="0.0000"
                                        step="0.0001"
                                        min="0.0001"
                                        max={Number(selectedWallet.balance)}
                                        className="w-full px-4 py-3 bg-white/5 border border-white/10 rounded-xl text-white placeholder-text-dim focus:outline-none focus:border-primary/50"
                                    />
                                    <div className="absolute right-4 top-1/2 -translate-y-1/2 text-xs text-text-dim">
                                        Available: {Number(selectedWallet.balance).toFixed(4)}
                                    </div>
                                </div>
                            </div>

                            <div>
                                <label className="block text-sm font-bold text-white mb-2">Recipient Address</label>
                                <input
                                    type="text"
                                    value={withdrawForm.address}
                                    onChange={(e) => setWithdrawForm({ ...withdrawForm, address: e.target.value })}
                                    placeholder="0x..."
                                    className="w-full px-4 py-3 bg-white/5 border border-white/10 rounded-xl text-white placeholder-text-dim focus:outline-none focus:border-primary/50 font-mono text-sm"
                                />
                            </div>

                            <div>
                                <label className="block text-sm font-bold text-white mb-2">2FA Code</label>
                                <input
                                    type="text"
                                    value={withdrawForm.twoFACode}
                                    onChange={(e) => setWithdrawForm({ ...withdrawForm, twoFACode: e.target.value })}
                                    placeholder="000000"
                                    maxLength={6}
                                    className="w-full px-4 py-3 bg-white/5 border border-white/10 rounded-xl text-white placeholder-text-dim focus:outline-none focus:border-primary/50 font-mono text-center text-lg"
                                />
                            </div>

                            <div className="bg-red-500/10 border border-red-500/20 rounded-xl p-4">
                                <div className="flex items-start gap-3">
                                    <AlertTriangle className="w-5 h-5 text-red-500 flex-shrink-0 mt-0.5" />
                                    <div>
                                        <p className="text-sm font-bold text-red-500 mb-1">Withdrawal Warning</p>
                                        <ul className="text-xs text-red-500/80 space-y-1">
                                            <li>• Withdrawals are irreversible</li>
                                            <li>• Daily limit: $500 USD equivalent</li>
                                            <li>• Network fees will be deducted</li>
                                            <li>• Double-check the recipient address</li>
                                        </ul>
                                    </div>
                                </div>
                            </div>

                            <button
                                onClick={handleWithdrawSubmit}
                                disabled={isSubmitting || !withdrawForm.amount || !withdrawForm.address || !withdrawForm.twoFACode}
                                className="w-full py-3 bg-primary text-white rounded-xl font-black uppercase tracking-wider text-sm hover:bg-primary/90 transition-colors disabled:opacity-50 disabled:cursor-not-allowed flex items-center justify-center gap-2"
                            >
                                {isSubmitting ? (
                                    <>
                                        <Loader2 className="w-4 h-4 animate-spin" />
                                        Processing...
                                    </>
                                ) : (
                                    <>
                                        <ArrowUpRight className="w-4 h-4" />
                                        Withdraw Funds
                                    </>
                                )}
                            </button>
                        </div>
                    </motion.div>
                </motion.div>
            )}

        </DashboardLayout>
    );
};

export default UserWallet;
