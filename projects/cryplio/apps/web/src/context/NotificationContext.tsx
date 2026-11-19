"use client";

import React, { createContext, useContext, useState, useEffect, ReactNode } from 'react';
import { wsService } from '@/services/websocketService';
import { toast } from 'sonner';
import { NotificationService } from "@/services/notificationService";

export interface Notification {
    id: string;
    title: string;
    message: string;
    type: "trade_update" | "system" | "payment_received" | "dispute_raised" | "trade_message";
    is_read: boolean;
    created_at: string;
    data?: any;
}

interface NotificationContextType {
    notifications: Notification[];
    unreadCount: number;
    addNotification: (notification: Partial<Notification>) => void;
    markAsRead: (id: string) => void;
    markAllAsRead: () => void;
    clearNotifications: () => void;
    isConnected: boolean;
}

const NotificationContext = createContext<NotificationContextType | undefined>(undefined);

export const useNotifications = () => {
    const context = useContext(NotificationContext);
    if (!context) {
        throw new Error('useNotifications must be used within NotificationProvider');
    }
    return context;
};

interface NotificationProviderProps {
    children: ReactNode;
}

export const NotificationProvider: React.FC<NotificationProviderProps> = ({ children }) => {
    const [notifications, setNotifications] = useState<Notification[]>([]);
    const [isConnected, setIsConnected] = useState(false);

    useEffect(() => {
        // Connect to WebSocket
        wsService.connect();

        // Listen for connection events
        wsService.on('connected', () => {
            setIsConnected(true);
        });

        wsService.on('disconnected', () => {
            setIsConnected(false);
        });

        // Listen for new messages (to show notification if user is not on chat page)
        wsService.on('new_message', (data) => {
            const payload = data.data || data;
            addNotification({
                type: 'trade_message',
                title: 'New Message',
                message: payload.message || 'You received a new message',
                data: { trade_id: payload.trade_id }
            });
        });

        // Listen for generic notification events (which wrap trade_update, dispute_created, etc.)
        wsService.on('notification', (event) => {
            const payload = event.data || event;
            addNotification({
                type: payload.type as any,
                title: payload.title || 'Notification',
                message: payload.message || '',
                data: payload.data
            });
        });

        // Legacy compatibility listeners
        wsService.on('trade_update', (data) => {
            const payload = data.data || data;
            addNotification({
                type: 'trade_update',
                title: 'Trade Update',
                message: payload.message || `Trade update for ${payload.trade_id}`,
                data: { trade_id: payload.trade_id }
            });
        });

        wsService.on('message', (data) => {
            const payload = data.data || data;
            addNotification({
                type: 'system',
                title: payload.title || 'Notification',
                message: payload.message || 'You have a new notification'
            });
        });

        // Fetch existing notifications
        const fetchNotifications = async () => {
            // Only fetch if we have a user_id (session might exist)
            if (typeof window !== 'undefined' && !localStorage.getItem('user_id')) {
                return;
            }

            try {
                const data = await NotificationService.getNotifications();
                setNotifications(data);
            } catch (error) {
                console.error("Failed to fetch notifications:", error);
            }
        };

        fetchNotifications();

        // Cleanup on unmount
        return () => {
            wsService.disconnect();
        };
    }, []);

    const addNotification = (notification: Partial<Notification>) => {
        const newNotification: Notification = {
            id: Date.now().toString() + Math.random().toString(36).substr(2, 9),
            title: notification.title || 'Notification',
            message: notification.message || '',
            type: notification.type || 'system',
            is_read: false,
            created_at: new Date().toISOString(),
            data: notification.data
        };

        setNotifications(prev => [newNotification, ...prev]);

        // Show toast notification
        const toastType = notification.type === 'trade_update' ? 'info' :
            notification.type === 'dispute_raised' ? 'error' : 'info';

        (toast as any)[toastType](newNotification.title, {
            description: newNotification.message,
        });
    };

    const markAsRead = async (id: string) => {
        try {
            await NotificationService.markAsRead(id);
            setNotifications(prev =>
                prev.map(notification =>
                    notification.id === id ? { ...notification, is_read: true } : notification
                )
            );
        } catch (error) {
            console.error("Failed to mark notification as read:", error);
        }
    };

    const markAllAsRead = () => {
        // Implementation for marking all as read would go here
        setNotifications(prev =>
            prev.map(notification => ({ ...notification, is_read: true }))
        );
    };

    const clearNotifications = () => {
        setNotifications([]);
    };

    const unreadCount = notifications.filter(n => !n.is_read).length;

    return (
        <NotificationContext.Provider
            value={{
                notifications,
                unreadCount,
                addNotification,
                markAsRead,
                markAllAsRead,
                clearNotifications,
                isConnected
            }}
        >
            {children}
        </NotificationContext.Provider>
    );
};
