class WebSocketService {
    private ws: WebSocket | null = null;
    private reconnectAttempts = 0;
    private maxReconnectAttempts = 20; // Increased
    private reconnectDelay = 1000;
    private listeners: Map<string, ((data: any) => void)[]> = new Map();
    private isConnecting = false;
    private messageQueue: any[] = [];

    connect() {
        if (this.isConnecting || (this.ws && (this.ws.readyState === WebSocket.OPEN || this.ws.readyState === WebSocket.CONNECTING))) {
            return;
        }

        const userId = this.getUserId();
        const token = this.getToken();

        if (!userId) {
            console.log('WebSocket: No user ID found, skipping connection');
            return;
        }

        this.isConnecting = true;
        const baseUrl = process.env.NEXT_PUBLIC_WS_URL || (typeof window !== 'undefined' ? `${window.location.protocol === 'https:' ? 'wss:' : 'ws:'}//${window.location.host}/ws` : 'ws://localhost:8080/ws');

        // Use token for authentication if available, fallback to user_id (though backend should enforce token)
        const wsUrl = token
            ? `${baseUrl}?token=${token}`
            : `${baseUrl}?user_id=${userId}`;

        try {
            console.log('Connecting to WebSocket...');
            this.ws = new WebSocket(wsUrl);

            this.ws.onopen = () => {
                console.log('WebSocket connected');
                this.isConnecting = false;
                this.reconnectAttempts = 0;
                this.emit('connected', { message: 'Connected to server' });

                // Send queued messages
                this.flushQueue();
            };

            this.ws.onmessage = (event) => {
                try {
                    const data = JSON.parse(event.data);
                    // Broadcast specific type if available, otherwise generic message
                    this.emit(data.type || 'message', data);
                } catch (error) {
                    console.error('Error parsing WebSocket message:', error);
                }
            };

            this.ws.onclose = (event) => {
                console.log('WebSocket disconnected:', event.code, event.reason);
                this.isConnecting = false;
                this.emit('disconnected', { code: event.code, reason: event.reason });

                if (this.reconnectAttempts < this.maxReconnectAttempts) {
                    this.scheduleReconnect();
                }
            };

            this.ws.onerror = (error) => {
                console.error('WebSocket error:', error);
                this.isConnecting = false;
                this.emit('error', { error: 'WebSocket connection error' });
            };

        } catch (error) {
            console.error('Failed to create WebSocket connection:', error);
            this.isConnecting = false;
        }
    }

    private getUserId(): string | null {
        if (typeof window !== 'undefined') {
            return localStorage.getItem('user_id');
        }
        return null;
    }

    private getToken(): string | null {
        if (typeof window !== 'undefined') {
            return localStorage.getItem('auth_token') || localStorage.getItem('token');
        }
        return null;
    }

    private scheduleReconnect() {
        this.reconnectAttempts++;
        // Exponential backoff with a cap of 30 seconds
        const delay = Math.min(this.reconnectDelay * Math.pow(2, this.reconnectAttempts - 1), 30000);

        console.log(`Attempting to reconnect in ${delay}ms (attempt ${this.reconnectAttempts})`);

        setTimeout(() => {
            this.connect();
        }, delay);
    }

    private flushQueue() {
        if (!this.ws || this.ws.readyState !== WebSocket.OPEN) return;

        while (this.messageQueue.length > 0) {
            const data = this.messageQueue.shift();
            this.ws.send(JSON.stringify(data));
        }
    }

    disconnect() {
        if (this.ws) {
            this.ws.close();
            this.ws = null;
        }
    }

    send(data: any) {
        if (this.ws && this.ws.readyState === WebSocket.OPEN) {
            this.ws.send(JSON.stringify(data));
        } else {
            console.log('WebSocket not ready, queuing message:', data.type);
            this.messageQueue.push(data);

            // If not connected and not connecting, try to connect
            if (!this.isConnecting && (!this.ws || this.ws.readyState === WebSocket.CLOSED)) {
                this.connect();
            }
        }
    }

    on(event: string, callback: (data: any) => void) {
        if (!this.listeners.has(event)) {
            this.listeners.set(event, []);
        }
        this.listeners.get(event)!.push(callback);
    }

    off(event: string, callback: (data: any) => void) {
        const callbacks = this.listeners.get(event);
        if (callbacks) {
            const index = callbacks.indexOf(callback);
            if (index > -1) {
                callbacks.splice(index, 1);
            }
        }
    }

    private emit(event: string, data: any) {
        const callbacks = this.listeners.get(event);
        if (callbacks) {
            callbacks.forEach(callback => {
                try {
                    callback(data);
                } catch (error) {
                    console.error(`Error in WebSocket callback for event "${event}":`, error);
                }
            });
        }
    }

    isConnected() {
        return this.ws && this.ws.readyState === WebSocket.OPEN;
    }
}

export const wsService = new WebSocketService();
