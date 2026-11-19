import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { ExchangeWebSocket } from './websocket';
import type { ConnectionStatus } from './websocket';

// ---------------------------------------------------------------------------
// MockWebSocket
// ---------------------------------------------------------------------------

class MockWebSocket {
  static instances: MockWebSocket[] = [];
  static OPEN = 1;
  static CONNECTING = 0;
  static CLOSING = 2;
  static CLOSED = 3;

  onopen: (() => void) | null = null;
  onmessage: ((e: { data: string }) => void) | null = null;
  onclose: (() => void) | null = null;
  onerror: ((e: unknown) => void) | null = null;
  readyState: number = 0; // CONNECTING
  sent: string[] = [];
  closeCode: number | undefined;
  closeReason: string | undefined;

  constructor(public url: string) {
    MockWebSocket.instances.push(this);
  }

  send(data: string): void {
    this.sent.push(data);
  }

  close(code?: number, reason?: string): void {
    this.closeCode = code;
    this.closeReason = reason;
    this.readyState = MockWebSocket.CLOSED;
  }

  // Test helpers

  simulateOpen(): void {
    this.readyState = MockWebSocket.OPEN;
    this.onopen?.();
  }

  simulateMessage(data: unknown): void {
    this.onmessage?.({ data: JSON.stringify(data) });
  }

  simulateClose(): void {
    this.readyState = MockWebSocket.CLOSED;
    this.onclose?.();
  }

  simulateError(): void {
    this.onerror?.({ type: 'error' });
  }

  /** Return parsed JSON messages (filtering out nulls) */
  parsedSent(): unknown[] {
    return this.sent.map((s) => JSON.parse(s));
  }
}

// ---------------------------------------------------------------------------
// Setup
// ---------------------------------------------------------------------------

beforeEach(() => {
  MockWebSocket.instances = [];
  vi.useFakeTimers();
  vi.stubGlobal('WebSocket', MockWebSocket);
  // jsdom sets location.protocol = 'http:' and location.host = 'localhost'
  Object.defineProperty(window, 'location', {
    value: { protocol: 'http:', host: 'localhost', hash: '' },
    writable: true,
  });
});

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function latestSocket(): MockWebSocket {
  const s = MockWebSocket.instances[MockWebSocket.instances.length - 1];
  if (!s) throw new Error('No MockWebSocket instances');
  return s;
}

function connectAndAuth(ws: ExchangeWebSocket): MockWebSocket {
  ws.connect();
  const sock = latestSocket();
  sock.simulateOpen();
  // Server sends auth success
  sock.simulateMessage({ op: 'auth', success: true });
  return sock;
}

// ---------------------------------------------------------------------------
// Connection
// ---------------------------------------------------------------------------

describe('ExchangeWebSocket — connection', () => {
  it('creates a WebSocket with the correct ws:// URL', () => {
    const ws = new ExchangeWebSocket();
    ws.connect();

    const sock = latestSocket();
    expect(sock.url).toBe('ws://localhost/ws/v1');
    ws.disconnect();
  });

  it('emits "connecting" status when connect() is called', () => {
    const ws = new ExchangeWebSocket();
    const statuses: ConnectionStatus[] = [];
    ws.onStatus((s) => statuses.push(s));

    ws.connect();
    expect(statuses).toContain('connecting');
    ws.disconnect();
  });

  it('sends auth op immediately after socket opens', () => {
    const ws = new ExchangeWebSocket();
    ws.connect();
    const sock = latestSocket();
    sock.simulateOpen();

    expect(sock.parsedSent()).toContainEqual({ op: 'auth' });
    ws.disconnect();
  });

  it('emits "connected" status after server confirms auth', () => {
    const ws = new ExchangeWebSocket();
    const statuses: ConnectionStatus[] = [];
    ws.onStatus((s) => statuses.push(s));

    connectAndAuth(ws);
    expect(statuses).toContain('connected');
    ws.disconnect();
  });

  it('emits "error" status when server rejects auth', () => {
    const ws = new ExchangeWebSocket();
    const statuses: ConnectionStatus[] = [];
    ws.onStatus((s) => statuses.push(s));

    ws.connect();
    const sock = latestSocket();
    sock.simulateOpen();
    sock.simulateMessage({ op: 'auth', success: false });

    expect(statuses).toContain('error');
    ws.disconnect();
  });

  it('does not reconnect after explicit disconnect()', () => {
    const ws = new ExchangeWebSocket();
    ws.connect();
    ws.disconnect();

    const countBefore = MockWebSocket.instances.length;
    vi.advanceTimersByTime(60_000);
    expect(MockWebSocket.instances.length).toBe(countBefore);
  });
});

// ---------------------------------------------------------------------------
// Subscriptions
// ---------------------------------------------------------------------------

describe('ExchangeWebSocket — subscriptions', () => {
  it('sends subscribe message after auth success', () => {
    const ws = new ExchangeWebSocket();
    ws.subscribe(['ticker.BTCUSDT']);
    const sock = connectAndAuth(ws);

    const msgs = sock.parsedSent();
    expect(msgs).toContainEqual({ op: 'subscribe', args: ['ticker.BTCUSDT'] });
    ws.disconnect();
  });

  it('buffers subscriptions added before connect and sends them after auth', () => {
    const ws = new ExchangeWebSocket();
    // Subscribe before ever calling connect
    ws.subscribe(['trade.BTCUSDT', 'orderbook.BTCUSDT']);
    const sock = connectAndAuth(ws);

    const msgs = sock.parsedSent();
    const subMsg = msgs.find(
      (m) => (m as { op: string }).op === 'subscribe',
    ) as { op: string; args: string[] } | undefined;

    expect(subMsg).toBeDefined();
    expect(subMsg!.args).toContain('trade.BTCUSDT');
    expect(subMsg!.args).toContain('orderbook.BTCUSDT');
    ws.disconnect();
  });

  it('subscribe() after auth sends immediately without buffering', () => {
    const ws = new ExchangeWebSocket();
    const sock = connectAndAuth(ws);
    const countBefore = sock.sent.length;

    ws.subscribe(['kline.ETHUSDT.1m']);

    expect(sock.sent.length).toBeGreaterThan(countBefore);
    expect(sock.parsedSent()).toContainEqual({ op: 'subscribe', args: ['kline.ETHUSDT.1m'] });
    ws.disconnect();
  });

  it('unsubscribe sends correct message when connected', () => {
    const ws = new ExchangeWebSocket();
    const sock = connectAndAuth(ws);

    ws.subscribe(['ticker.BTCUSDT']);
    ws.unsubscribe(['ticker.BTCUSDT']);

    const msgs = sock.parsedSent();
    expect(msgs).toContainEqual({ op: 'unsubscribe', args: ['ticker.BTCUSDT'] });
    ws.disconnect();
  });

  it('subscribe before connection does not send until open+auth', () => {
    const ws = new ExchangeWebSocket();
    ws.subscribe(['ticker.SOLUSDT']);

    ws.connect();
    const sock = latestSocket();
    // Socket is open but auth has NOT been confirmed yet
    sock.simulateOpen();

    // Only the auth op should have been sent — no subscribe yet
    const msgs = sock.parsedSent();
    const subscribeMsgs = msgs.filter((m) => (m as { op: string }).op === 'subscribe');
    expect(subscribeMsgs).toHaveLength(0);

    ws.disconnect();
  });
});

// ---------------------------------------------------------------------------
// Message dispatching
// ---------------------------------------------------------------------------

describe('ExchangeWebSocket — message dispatching', () => {
  it('calls message handlers when server sends data', () => {
    const ws = new ExchangeWebSocket();
    const received: unknown[] = [];
    ws.onMessage((msg) => received.push(msg));

    const sock = connectAndAuth(ws);
    const payload = { op: 'data', topic: 'ticker.BTCUSDT', data: { last_price: '65000' } };
    sock.simulateMessage(payload);

    expect(received).toContainEqual(payload);
    ws.disconnect();
  });

  it('silently ignores messages that are not valid JSON', () => {
    const ws = new ExchangeWebSocket();
    const received: unknown[] = [];
    ws.onMessage((msg) => received.push(msg));

    ws.connect();
    const sock = latestSocket();
    sock.simulateOpen();
    // Malformed JSON — should not throw
    expect(() => sock.onmessage?.({ data: '{{not json}}' })).not.toThrow();
    expect(received).toHaveLength(0);
    ws.disconnect();
  });

  it('onMessage returns an unsubscribe function that stops future deliveries', () => {
    const ws = new ExchangeWebSocket();
    // Connect and auth first so the handler is registered after auth messages
    const sock = connectAndAuth(ws);

    const received: unknown[] = [];
    const unsub = ws.onMessage((msg) => received.push(msg));
    unsub(); // deregister immediately

    sock.simulateMessage({ op: 'pong' });
    expect(received).toHaveLength(0);
    ws.disconnect();
  });
});

// ---------------------------------------------------------------------------
// Reconnect
// ---------------------------------------------------------------------------

describe('ExchangeWebSocket — reconnect', () => {
  it('schedules a reconnect after connection closes unexpectedly', () => {
    const ws = new ExchangeWebSocket();
    connectAndAuth(ws);
    const countBefore = MockWebSocket.instances.length;

    latestSocket().simulateClose();
    // Advance past RECONNECT_DELAY_MS (2000 ms)
    vi.advanceTimersByTime(2100);

    expect(MockWebSocket.instances.length).toBeGreaterThan(countBefore);
    ws.disconnect();
  });

  it('resubscribes to all topics after reconnect', () => {
    const ws = new ExchangeWebSocket();
    ws.subscribe(['ticker.BTCUSDT', 'trade.ETHUSDT', 'orderbook.SOLUSDT']);
    connectAndAuth(ws);

    // Simulate disconnect + reconnect
    latestSocket().simulateClose();
    vi.advanceTimersByTime(2100);

    const newSock = latestSocket();
    newSock.simulateOpen();
    newSock.simulateMessage({ op: 'auth', success: true });

    const subscribeMsgs = newSock
      .parsedSent()
      .filter((m) => (m as { op: string }).op === 'subscribe') as { op: string; args: string[] }[];

    const allArgs = subscribeMsgs.flatMap((m) => m.args);
    expect(allArgs).toContain('ticker.BTCUSDT');
    expect(allArgs).toContain('trade.ETHUSDT');
    expect(allArgs).toContain('orderbook.SOLUSDT');
    ws.disconnect();
  });

  it('emits "disconnected" status when connection closes', () => {
    const ws = new ExchangeWebSocket();
    const statuses: ConnectionStatus[] = [];
    ws.onStatus((s) => statuses.push(s));

    connectAndAuth(ws);
    latestSocket().simulateClose();

    expect(statuses).toContain('disconnected');
    ws.disconnect();
  });

  it('emits "connecting" status on reconnect attempt', () => {
    const ws = new ExchangeWebSocket();
    const statuses: ConnectionStatus[] = [];
    ws.onStatus((s) => statuses.push(s));

    connectAndAuth(ws);
    latestSocket().simulateClose();
    vi.advanceTimersByTime(2100);

    // After the timer fires, a new openSocket() is called → 'connecting' emitted again
    const connectingCount = statuses.filter((s) => s === 'connecting').length;
    expect(connectingCount).toBeGreaterThanOrEqual(2);
    ws.disconnect();
  });
});

// ---------------------------------------------------------------------------
// Ping
// ---------------------------------------------------------------------------

describe('ExchangeWebSocket — ping', () => {
  it('sends ping after PING_INTERVAL_MS (20 000 ms)', () => {
    const ws = new ExchangeWebSocket();
    const sock = connectAndAuth(ws);
    const countBefore = sock.sent.length;

    vi.advanceTimersByTime(20_001);

    const newMsgs = sock.parsedSent().slice(countBefore);
    expect(newMsgs).toContainEqual({ op: 'ping' });
    ws.disconnect();
  });

  it('sends multiple pings over time', () => {
    const ws = new ExchangeWebSocket();
    const sock = connectAndAuth(ws);
    const countBefore = sock.sent.length;

    vi.advanceTimersByTime(60_001); // 3 × 20s intervals

    const pings = sock.parsedSent().slice(countBefore).filter((m) => (m as { op: string }).op === 'ping');
    expect(pings.length).toBeGreaterThanOrEqual(3);
    ws.disconnect();
  });
});

// ---------------------------------------------------------------------------
// Status handler lifecycle
// ---------------------------------------------------------------------------

describe('ExchangeWebSocket — status handler lifecycle', () => {
  it('onStatus returns an unsubscribe function that stops status deliveries', () => {
    const ws = new ExchangeWebSocket();
    const statuses: ConnectionStatus[] = [];
    const unsub = ws.onStatus((s) => statuses.push(s));
    unsub();

    ws.connect();
    expect(statuses).toHaveLength(0);
    ws.disconnect();
  });

  it('full connection lifecycle emits connecting → connected → disconnected', () => {
    const ws = new ExchangeWebSocket();
    const statuses: ConnectionStatus[] = [];
    ws.onStatus((s) => statuses.push(s));

    connectAndAuth(ws);
    latestSocket().simulateClose();

    expect(statuses[0]).toBe('connecting');
    expect(statuses).toContain('connected');
    expect(statuses[statuses.length - 1]).toBe('disconnected');
    ws.disconnect();
  });
});
