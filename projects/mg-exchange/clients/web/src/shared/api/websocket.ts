import type { WsOutgoing, WsIncoming } from './types';

export type ConnectionStatus = 'connecting' | 'connected' | 'disconnected' | 'error';

export type MessageHandler = (msg: WsIncoming) => void;
export type StatusHandler = (status: ConnectionStatus) => void;

// Use the Vite dev-server proxy path so the WS goes through /ws
const WS_URL = `/ws/v1`;
const RECONNECT_DELAY_MS = 2000;
const MAX_RECONNECT_DELAY_MS = 30_000;
const PING_INTERVAL_MS = 20_000;

/**
 * WebSocket client with auto-reconnect and topic subscription tracking.
 *
 * Usage:
 *   const ws = new ExchangeWebSocket();
 *   ws.onStatus(status => setStatus(status));
 *   ws.onMessage(msg => handleMsg(msg));
 *   ws.connect();
 *   ws.subscribe(['trade.BTCUSDT', 'orderbook.BTCUSDT']);
 *   // later:
 *   ws.disconnect();
 */
export class ExchangeWebSocket {
  private socket: WebSocket | null = null;
  private subscriptions: Set<string> = new Set();
  private authenticated = false;
  private reconnectDelay = RECONNECT_DELAY_MS;
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  private pingTimer: ReturnType<typeof setInterval> | null = null;
  private destroyed = false;

  private messageHandlers: Set<MessageHandler> = new Set();
  private statusHandlers: Set<StatusHandler> = new Set();

  private currentStatus: ConnectionStatus = 'disconnected';

  onMessage(handler: MessageHandler): () => void {
    this.messageHandlers.add(handler);
    return () => this.messageHandlers.delete(handler);
  }

  onStatus(handler: StatusHandler): () => void {
    this.statusHandlers.add(handler);
    return () => this.statusHandlers.delete(handler);
  }

  getStatus(): ConnectionStatus {
    return this.currentStatus;
  }

  connect(): void {
    if (this.destroyed) return;
    this.openSocket();
  }

  disconnect(): void {
    this.destroyed = true;
    this.clearTimers();
    if (this.socket) {
      this.socket.close(1000, 'Client disconnect');
      this.socket = null;
    }
  }

  subscribe(topics: string[]): void {
    for (const t of topics) this.subscriptions.add(t);

    if (this.socket?.readyState === WebSocket.OPEN && this.authenticated) {
      this.send({ op: 'subscribe', args: topics });
    }
  }

  unsubscribe(topics: string[]): void {
    for (const t of topics) this.subscriptions.delete(t);

    if (this.socket?.readyState === WebSocket.OPEN && this.authenticated) {
      this.send({ op: 'unsubscribe', args: topics });
    }
  }

  private openSocket(): void {
    this.emitStatus('connecting');

    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    const url = `${protocol}//${window.location.host}${WS_URL}`;

    try {
      this.socket = new WebSocket(url);
    } catch {
      this.scheduleReconnect();
      return;
    }

    this.socket.onopen = () => {
      this.reconnectDelay = RECONNECT_DELAY_MS;
      // Phase 1: auth without credentials (server accepts nil user)
      this.send({ op: 'auth' });
    };

    this.socket.onmessage = (ev: MessageEvent<string>) => {
      let msg: WsIncoming;
      try {
        msg = JSON.parse(ev.data) as WsIncoming;
      } catch {
        return;
      }

      if (msg.op === 'auth') {
        if (msg.success) {
          this.authenticated = true;
          this.emitStatus('connected');
          if (this.subscriptions.size > 0) {
            this.send({ op: 'subscribe', args: [...this.subscriptions] });
          }
          this.startPing();
        } else {
          this.emitStatus('error');
        }
      }

      for (const h of this.messageHandlers) h(msg);
    };

    this.socket.onclose = () => {
      this.authenticated = false;
      this.clearTimers();
      if (!this.destroyed) {
        this.emitStatus('disconnected');
        this.scheduleReconnect();
      }
    };

    this.socket.onerror = () => {
      this.emitStatus('error');
      // onclose fires right after onerror, so reconnect happens there
    };
  }

  private send(msg: WsOutgoing): void {
    if (this.socket?.readyState === WebSocket.OPEN) {
      this.socket.send(JSON.stringify(msg));
    }
  }

  private scheduleReconnect(): void {
    if (this.destroyed) return;
    this.reconnectTimer = setTimeout(() => {
      this.reconnectDelay = Math.min(this.reconnectDelay * 2, MAX_RECONNECT_DELAY_MS);
      this.openSocket();
    }, this.reconnectDelay);
  }

  private startPing(): void {
    this.pingTimer = setInterval(() => {
      this.send({ op: 'ping' });
    }, PING_INTERVAL_MS);
  }

  private clearTimers(): void {
    if (this.reconnectTimer !== null) {
      clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }
    if (this.pingTimer !== null) {
      clearInterval(this.pingTimer);
      this.pingTimer = null;
    }
  }

  private emitStatus(status: ConnectionStatus): void {
    this.currentStatus = status;
    for (const h of this.statusHandlers) h(status);
  }
}

// Singleton instance for app-wide use
export const ws = new ExchangeWebSocket();
