"use client";

import React, { useState, useEffect } from "react";
import DashboardLayout from "@/components/dashboard/DashboardLayout";
import MyListings from "@/components/dashboard/MyListings";
import { MarketplaceService, AdResponse } from "@/services/marketplaceService";
import { Loader2 } from "lucide-react";

const MyPostingsPage = () => {
    const [ads, setAds] = useState<AdResponse[]>([]);
    const [isLoading, setIsLoading] = useState(true);

    const fetchAds = async () => {
        setIsLoading(true);
        try {
            const data = await MarketplaceService.getMyAds();
            setAds(data || []);
        } catch (error) {
            console.error("Failed to fetch ads:", error);
        } finally {
            setIsLoading(false);
        }
    };

    useEffect(() => {
        fetchAds();
    }, []);

    return (
        <DashboardLayout title="My Postings" role="user">
            <div className="space-y-8">
                <div>
                    <h1 className="text-3xl font-black text-white italic uppercase tracking-tight mb-2">My Advertisements</h1>
                    <p className="text-text-dim text-xs font-bold uppercase tracking-[0.2em]">Manage your active buy and sell listings</p>
                </div>

                {isLoading ? (
                    <div className="flex items-center justify-center min-h-[40vh]">
                        <Loader2 className="w-10 h-10 text-primary animate-spin" />
                    </div>
                ) : (
                    <MyListings ads={ads} onRefresh={fetchAds} />
                )}
            </div>
        </DashboardLayout>
    );
};

export default MyPostingsPage;
