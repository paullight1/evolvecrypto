"use client";

import React, { useState, useEffect, useRef } from "react";
import { motion, AnimatePresence } from "framer-motion";
import {
    Send,
    Paperclip,
    Download,
    Image,
    FileText,
    X,
    Check,
    CheckCheck,
    Loader2
} from "lucide-react";
import { wsService } from "@/services/websocketService";
import { TradeService } from "@/services/tradeService";
import { toast } from "sonner";

interface ChatMessage {
    id: string;
    trade_id: string;
    sender_id: string;
    content?: string;
    file_url?: string;
    mime_type?: string;
    file_size?: number;
    created_at: string;
    sender_username?: string;
    is_system?: boolean;
}

interface TradeChatProps {
    tradeId: string;
    currentUserId: string;
    counterpartUsername?: string;
}

const TradeChat: React.FC<TradeChatProps> = ({
    tradeId,
    currentUserId,
    counterpartUsername
}) => {
    const [messages, setMessages] = useState<ChatMessage[]>([]);
    const [newMessage, setNewMessage] = useState("");
    const [isLoading, setIsLoading] = useState(true);
    const [isSending, setIsSending] = useState(false);
    const [isUploading, setIsUploading] = useState(false);
    const [attachedFile, setAttachedFile] = useState<File | null>(null);
    const [filePreview, setFilePreview] = useState<string | null>(null);
    const fileInputRef = useRef<HTMLInputElement>(null);
    const chatContainerRef = useRef<HTMLDivElement>(null);

    const scrollToBottom = () => {
        if (chatContainerRef.current) {
            chatContainerRef.current.scrollTop = chatContainerRef.current.scrollHeight;
        }
    };

    useEffect(() => {
        // Use a slight timeout to ensure content is fully rendered before scrolling
        const timeoutId = setTimeout(scrollToBottom, 100);
        return () => clearTimeout(timeoutId);
    }, [messages]);

    useEffect(() => {
        fetchChatHistory();
        subscribeToTradeChat();

        return () => {
            unsubscribeFromTradeChat();
        };
    }, [tradeId]);



    const fetchChatHistory = async () => {
        try {
            const data = await TradeService.getTradeMessages(tradeId);
            setMessages(data || []);
        } catch (error) {
            console.error("Error fetching chat history:", error);
            toast.error("Failed to load chat history");
        } finally {
            setIsLoading(false);
        }
    };

    const subscribeToTradeChat = () => {
        // Subscribe to trade chat messages
        wsService.send({
            type: "subscribe_trade",
            trade_id: tradeId
        });

        // Listen for chat messages
        wsService.on("chat_message", (eventData: any) => {
            // The actual message payload is nested in eventData.data
            const payload = eventData.data;

            if (eventData.trade_id === tradeId || payload?.trade_id === tradeId) {
                // Ensure field consistency
                const formattedMessage: ChatMessage = {
                    ...payload,
                    content: payload.content || payload.message,
                    created_at: payload.created_at || eventData.timestamp || new Date().toISOString()
                };

                setMessages(prev => {
                    // Avoid duplicate messages
                    if (prev.some(m => m.id === formattedMessage.id)) {
                        return prev;
                    }
                    return [...prev, formattedMessage];
                });
            }
        });
    };

    const unsubscribeFromTradeChat = () => {
        wsService.send({
            type: "unsubscribe_trade",
            trade_id: tradeId
        });
    };



    const sendMessage = async () => {
        if (!newMessage.trim() && !attachedFile) return;

        setIsSending(true);
        try {
            if (attachedFile) {
                await sendFileMessage(attachedFile, newMessage.trim());
                setAttachedFile(null);
                if (filePreview) {
                    URL.revokeObjectURL(filePreview);
                    setFilePreview(null);
                }
            } else {
                await TradeService.sendTradeMessage(tradeId, newMessage.trim());
            }
            setNewMessage("");
        } catch (error) {
            console.error("Error sending message:", error);
            toast.error("Failed to send message");
        } finally {
            setIsSending(false);
        }
    };

    const sendFileMessage = async (file: File, content?: string) => {
        setIsUploading(true);
        try {
            await TradeService.sendFileMessage(tradeId, file, content);
            toast.success("File sent successfully");
        } catch (error: any) {
            console.error("Error sending file:", error);
            toast.error(error.message || "Failed to send file");
        } finally {
            setIsUploading(false);
        }
    };

    const handleFileSelect = (event: React.ChangeEvent<HTMLInputElement>) => {
        const file = event.target.files?.[0];
        if (file) {
            // Check file size (5MB limit to match backend)
            if (file.size > 5 * 1024 * 1024) {
                toast.error("File size must be less than 5MB");
                event.target.value = "";
                return;
            }

            // Cleanup previous preview
            if (filePreview) {
                URL.revokeObjectURL(filePreview);
            }

            setAttachedFile(file);

            // Create preview if it's an image
            if (file.type.startsWith("image/")) {
                const previewUrl = URL.createObjectURL(file);
                setFilePreview(previewUrl);
            } else {
                setFilePreview(null);
            }

            // Clear input so same file can be selected again
            event.target.value = "";
        }
    };

    const formatTime = (timestamp: string) => {
        if (!timestamp) return "";
        const date = new Date(timestamp);
        if (isNaN(date.getTime())) return "";

        return date.toLocaleTimeString([], {
            hour: "2-digit",
            minute: "2-digit"
        });
    };

    const isOwnMessage = (message: ChatMessage) => {
        return message.sender_id === currentUserId;
    };

    const getFileIcon = (mimeType?: string) => {
        if (!mimeType) return FileText;

        if (mimeType.startsWith("image/")) {
            return Image;
        }

        return FileText;
    };

    const formatFileSize = (bytes?: number) => {
        if (!bytes) return "";

        const sizes = ["B", "KB", "MB", "GB"];
        const i = Math.floor(Math.log(bytes) / Math.log(1024));
        return Math.round(bytes / Math.pow(1024, i) * 100) / 100 + " " + sizes[i];
    };

    if (isLoading) {
        return (
            <div className="flex items-center justify-center h-64">
                <Loader2 className="w-8 h-8 animate-spin text-primary" />
            </div>
        );
    }

    return (
        <div className="flex flex-col h-full bg-surface border border-white/10 rounded-2xl">
            {/* Chat Header */}
            <div className="flex items-center justify-between p-4 border-b border-white/10">
                <div>
                    <h3 className="text-sm font-bold text-white">Trade Chat</h3>
                    <p className="text-xs text-text-dim">
                        {counterpartUsername ? `Chat with ${counterpartUsername}` : "Chat with trade partner"}
                    </p>
                </div>
                <div className="flex items-center space-x-2">
                    <div className="w-2 h-2 bg-green-400 rounded-full animate-pulse" />
                    <span className="text-xs text-text-dim">Connected</span>
                </div>
            </div>

            {/* Messages */}
            <div
                ref={chatContainerRef}
                className="flex-1 overflow-y-auto p-4 space-y-4"
            >
                <AnimatePresence>
                    {messages.map((message, index) => {
                        const FileIcon = getFileIcon(message.mime_type);
                        const isOwn = isOwnMessage(message);

                        // Skip rendering empty messages that have neither content nor files
                        if (!message.content && !message.file_url && !message.is_system) {
                            return null;
                        }

                        return (
                            <motion.div
                                key={message.id || `msg-${index}`}
                                initial={{ opacity: 0, y: 20 }}
                                animate={{ opacity: 1, y: 0 }}
                                className={`flex ${isOwn ? "justify-end" : "justify-start"}`}
                            >
                                <div className={`max-w-xs lg:max-w-md ${isOwn ? "order-2" : "order-1"
                                    }`}>
                                    <div className={`px-4 py-2 rounded-2xl ${isOwn
                                        ? "bg-primary text-white"
                                        : "bg-white/10 text-white"
                                        }`}>
                                        {message.content && (
                                            <p className="text-sm whitespace-pre-wrap break-words">
                                                {message.content}
                                            </p>
                                        )}

                                        {message.file_url && (
                                            <div className={`mt-2 ${message.mime_type?.startsWith("image/") ? "" : "bg-black/20 p-3 rounded-xl border border-white/5"}`}>
                                                {message.mime_type?.startsWith("image/") ? (
                                                    <div className="relative group overflow-hidden rounded-xl border border-white/10 mb-2">
                                                        <img
                                                            src={message.file_url}
                                                            alt="Attached image"
                                                            className="max-w-full h-auto max-h-60 object-contain rounded-xl hover:scale-[1.02] transition-transform duration-300"
                                                        />
                                                        <a
                                                            href={message.file_url}
                                                            target="_blank"
                                                            rel="noopener noreferrer"
                                                            className="absolute inset-0 bg-black/40 opacity-0 group-hover:opacity-100 flex items-center justify-center transition-opacity rounded-xl"
                                                        >
                                                            <Download className="w-6 h-6 text-white" />
                                                        </a>
                                                    </div>
                                                ) : (
                                                    <div className="flex items-center space-x-3">
                                                        <div className="p-2 bg-primary/20 rounded-lg">
                                                            <FileIcon className="w-5 h-5 text-primary" />
                                                        </div>
                                                        <div className="flex-1 min-w-0">
                                                            <p className="text-xs font-medium text-white truncate">
                                                                {message.file_url.split("/").pop()}
                                                            </p>
                                                            {message.file_size && (
                                                                <p className="text-[10px] text-text-dim mt-0.5">
                                                                    {formatFileSize(message.file_size)}
                                                                </p>
                                                            )}
                                                        </div>
                                                        <a
                                                            href={message.file_url}
                                                            target="_blank"
                                                            rel="noopener noreferrer"
                                                            className="p-2 hover:bg-white/10 rounded-lg transition-colors text-text-dim hover:text-white"
                                                        >
                                                            <Download className="w-4 h-4" />
                                                        </a>
                                                    </div>
                                                )}
                                            </div>
                                        )}

                                        <div className={`flex items-center justify-end space-x-1 mt-1 ${isOwn ? "text-primary-100" : "text-text-dim"
                                            }`}>
                                            <span className="text-xs">
                                                {formatTime(message.created_at)}
                                            </span>
                                            {isOwn && (
                                                <CheckCheck className="w-3 h-3" />
                                            )}
                                        </div>
                                    </div>
                                </div>
                            </motion.div>
                        );
                    })}
                </AnimatePresence>
            </div>

            {/* Message Input */}
            <div className="p-4 border-t border-white/10">
                {attachedFile && (
                    <div className="mb-2 p-2 bg-white/5 border border-white/10 rounded-xl flex flex-col space-y-2">
                        <div className="flex items-center justify-between">
                            <div className="flex items-center space-x-2 min-w-0">
                                <Paperclip className="w-4 h-4 text-primary shrink-0" />
                                <span className="text-xs text-white truncate">{attachedFile.name}</span>
                                <span className="text-[10px] text-text-dim shrink-0">({formatFileSize(attachedFile.size)})</span>
                            </div>
                            <button
                                onClick={() => {
                                    setAttachedFile(null);
                                    if (filePreview) {
                                        URL.revokeObjectURL(filePreview);
                                        setFilePreview(null);
                                    }
                                }}
                                className="p-1 hover:bg-white/10 rounded-full transition-colors"
                            >
                                <X className="w-3 h-3 text-text-dim" />
                            </button>
                        </div>
                        {filePreview && (
                            <div className="relative w-20 h-20 rounded-lg overflow-hidden border border-white/10">
                                <img src={filePreview} alt="Preview" className="w-full h-full object-cover" />
                            </div>
                        )}
                    </div>
                )}

                <div className="flex items-center space-x-2">
                    <input
                        type="file"
                        ref={fileInputRef}
                        onChange={handleFileSelect}
                        className="hidden"
                        accept="image/*,.pdf,.doc,.docx,.txt"
                    />

                    <button
                        onClick={() => fileInputRef.current?.click()}
                        disabled={isUploading || isSending}
                        className="p-2 text-text-dim hover:text-white transition-colors disabled:opacity-50"
                        title="Attach file"
                    >
                        {isUploading ? (
                            <Loader2 className="w-5 h-5 animate-spin" />
                        ) : (
                            <Paperclip className="w-5 h-5" />
                        )}
                    </button>

                    <input
                        type="text"
                        value={newMessage}
                        onChange={(e) => setNewMessage(e.target.value)}
                        onKeyPress={(e) => e.key === "Enter" && sendMessage()}
                        placeholder={attachedFile ? "Add a caption..." : "Type a message..."}
                        className="flex-1 px-4 py-2 bg-white/10 border border-white/10 rounded-xl text-white placeholder-text-dim focus:outline-none focus:border-primary/50"
                        disabled={isSending}
                    />

                    <button
                        onClick={sendMessage}
                        disabled={(!newMessage.trim() && !attachedFile) || isSending}
                        className="p-2 bg-primary text-white rounded-xl hover:bg-primary/90 transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
                    >
                        {isSending ? (
                            <Loader2 className="w-5 h-5 animate-spin" />
                        ) : (
                            <Send className="w-5 h-5" />
                        )}
                    </button>
                </div>

                <div className="mt-2 text-xs text-text-dim">
                    Files: Images, PDF, DOC, TXT (Max 10MB)
                </div>
            </div>
        </div>
    );
};

export default TradeChat;
