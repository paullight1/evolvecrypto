import { useEffect, useCallback, useRef } from 'react';
import { ws } from '@shared/api/websocket';
import { toast } from '@shared/ui/Toast';
import type { WsIncoming } from '@shared/api/types';

/**
 * Subscribe to private WebSocket channels (orders, fills, positions).
 * Shows toast notifications for order fills and rejections.
 * Calls `onUpdate` when positions or orders change so the caller can refetch.
 */
export function usePrivateWs(onUpdate?: () => void) {
  const onUpdateRef = useRef(onUpdate);
  onUpdateRef.current = onUpdate;

  const handleMessage = useCallback((msg: WsIncoming) => {
    if (!('topic' in msg) || !('data' in msg)) return;
    const topic = (msg as unknown as { topic: string }).topic;
    const data = (msg as unknown as { data: Record<string, unknown> }).data;

    if (topic === 'fills') {
      toast.success(`Filled: ${data.symbol} ${data.side} ${data.qty} @ ${data.price}`);
      onUpdateRef.current?.();
    }

    if (topic === 'orders') {
      if (data.status === 'rejected') {
        toast.error(`Order rejected: ${data.reason || 'Unknown'}`);
      }
      onUpdateRef.current?.();
    }

    if (topic === 'positions') {
      onUpdateRef.current?.();
    }
  }, []);

  useEffect(() => {
    const topics = ['orders', 'fills', 'positions'];
    ws.subscribe(topics);

    const unsub = ws.onMessage(handleMessage);

    return () => {
      ws.unsubscribe(topics);
      unsub();
    };
  }, [handleMessage]);
}
