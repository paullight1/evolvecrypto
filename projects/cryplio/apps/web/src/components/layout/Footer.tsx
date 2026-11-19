import React from "react";
import Link from "next/link";
import { Wallet, Send, Code2, Briefcase, MessageCircle, Terminal, Shield, Activity } from "lucide-react";

const Footer = () => {
    const footerLinks = [
        {
            title: "EXCHANGE",
            links: [
                { name: "P2P Marketplace", href: "/marketplace" },
                { name: "Institutional OTC", href: "#" },
                { name: "Mesh Swap", href: "/swap" },
                { name: "Fee Schedule", href: "#" },
            ],
        },
        {
            title: "PROTOCOL",
            links: [
                { name: "Security Audit", href: "/security" },
                { name: "Proof of Reserves", href: "#" },
                { name: "Developer SDK", href: "/api" },
                { name: "Whitepaper", href: "#" },
            ],
        },
        {
            title: "RESOURCES",
            links: [
                { name: "Command Center", href: "/support" },
                { name: "Status Page", href: "#" },
                { name: "Compliance", href: "#" },
                { name: "Registry", href: "#" },
            ],
        },
    ];

    return (
        <footer className="bg-[#050505] border-t border-white/5 pt-32 pb-16 overflow-hidden relative">
            {/* Background Texture & Gradients */}
            <div className="absolute top-0 left-1/4 w-[600px] h-[600px] bg-primary/5 rounded-full blur-[150px] pointer-events-none -z-10" />
            <div className="absolute bottom-0 right-1/4 w-[600px] h-[600px] bg-secondary/5 rounded-full blur-[150px] pointer-events-none -z-10" />
            <div className="absolute inset-0 bg-[url('/noise.png')] opacity-[0.02] pointer-events-none" />

            <div className="container mx-auto px-10 relative z-10">
                <div className="grid grid-cols-1 md:grid-cols-12 gap-20 mb-32">
                    {/* Brand Column */}
                    <div className="md:col-span-4 space-y-10">
                        <Link href="/" className="flex items-center space-x-4 group">
                            <div className="w-12 h-12 bg-gradient-to-br from-primary to-secondary rounded-2xl flex items-center justify-center shadow-2xl shadow-primary/20 transition-transform duration-500 group-hover:rotate-[15deg]">
                                <Wallet className="text-white w-6 h-6" />
                            </div>
                            <div className="flex flex-col">
                                <span className="text-2xl font-black italic uppercase tracking-tighter text-white leading-none">
                                    CRYP<span className="text-primary">LIO</span>
                                </span>
                                <span className="text-[8px] font-black tracking-[0.5em] text-white/30 uppercase mt-1">Institutional Grade</span>
                            </div>
                        </Link>
                        <p className="text-text-dim/60 text-xs font-bold uppercase tracking-[0.2em] leading-[2] max-w-sm italic">
                            The decentralized settlement gateway for professional markets. 100% Escrow protected, sub-second precision.
                        </p>

                        <div className="flex items-center gap-6">
                            <div className="px-6 py-3 bg-white/[0.03] border border-white/5 rounded-2xl flex items-center gap-3 hover:bg-white/[0.06] transition-colors group">
                                <div className="w-2 h-2 bg-primary rounded-full animate-pulse shadow-[0_0_10px_rgba(var(--primary-rgb),0.5)]" />
                                <span className="text-[10px] font-black text-white/40 uppercase tracking-widest group-hover:text-white transition-colors">CRYP-OS: LIVE</span>
                            </div>
                            <div className="px-6 py-3 bg-white/[0.03] border border-white/5 rounded-2xl flex items-center gap-3 hover:bg-white/[0.06] transition-colors group">
                                <Shield className="w-4 h-4 text-white/20 group-hover:text-primary transition-colors" />
                                <span className="text-[10px] font-black text-white/40 uppercase tracking-widest group-hover:text-white transition-colors">v2.4.0-STABLE</span>
                            </div>
                        </div>
                    </div>

                    {/* Links Columns */}
                    <div className="md:col-span-8 grid grid-cols-1 sm:grid-cols-3 gap-16">
                        {footerLinks.map((group) => (
                            <div key={group.title} className="space-y-10">
                                <h3 className="text-white text-[11px] font-black uppercase tracking-[0.4em] italic opacity-40">{group.title}</h3>
                                <ul className="space-y-5">
                                    {group.links.map((link) => (
                                        <li key={link.name}>
                                            <Link
                                                href={link.href}
                                                className="text-[11px] font-black text-text-dim/40 uppercase tracking-[0.3em] hover:text-white transition-all flex items-center gap-3 group"
                                            >
                                                <div className="w-1.5 h-1.5 rounded-full bg-primary/20 group-hover:bg-primary transition-colors group-hover:scale-150" />
                                                {link.name}
                                            </Link>
                                        </li>
                                    ))}
                                </ul>
                            </div>
                        ))}
                    </div>
                </div>

                <div className="pt-16 border-t border-white/5 flex flex-col lg:flex-row justify-between items-center gap-10">
                    <div className="order-2 lg:order-1">
                        <p className="text-[9px] font-black text-text-dim/30 uppercase tracking-[0.4em]">
                            © {new Date().getFullYear()} CRYPLIO PROTOCOL AG. ALL SYSTEMS OPERATIONAL.
                        </p>
                    </div>

                    <div className="flex flex-wrap justify-center items-center gap-10 order-1 lg:order-2">
                        {["Twitter", "Discord", "GitHub", "Telegram"].map(social => (
                            <Link key={social} href="#" className="text-[10px] font-black text-text-dim/40 hover:text-white uppercase tracking-[0.4em] transition-all hover:scale-110">
                                {social}
                            </Link>
                        ))}
                    </div>

                    <div className="flex items-center gap-10 text-[9px] font-black text-text-dim/30 uppercase tracking-[0.4em] order-3">
                        <Link href="#" className="hover:text-white transition-all">Privacy_Vault</Link>
                        <Link href="#" className="hover:text-white transition-all">Manifesto_Terms</Link>
                    </div>
                </div>
            </div>

            {/* Terminal Decoration */}
            <div className="absolute -bottom-10 right-10 flex items-center gap-2 opacity-10">
                <Terminal className="w-4 h-4" />
                <span className="text-[10px] font-mono tracking-tighter">SECURE_UPLINK_ESTABLISHED // PORT_443_SSL_V3</span>
            </div>
        </footer>
    );
};

export default Footer;
