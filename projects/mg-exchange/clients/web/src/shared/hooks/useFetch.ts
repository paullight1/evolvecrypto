import { useState, useEffect, useCallback, useRef } from 'react';
import { ApiError } from '@shared/api/client';

export interface FetchState<T> {
  data: T | null;
  loading: boolean;
  error: string | null;
  refetch: () => void;
}

/**
 * Generic data-fetching hook with loading/error state and manual refetch.
 *
 * The fetchFn reference is captured on mount and when the caller passes
 * a new function reference (wrap in useCallback to control re-fetch timing).
 *
 * Usage:
 *   const { data, loading, error, refetch } = useFetch(() => get<Order[]>('/orders'));
 */
export function useFetch<T>(fetchFn: () => Promise<T>): FetchState<T> {
  const [data, setData] = useState<T | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [tick, setTick] = useState(0);

  // Keep a ref to the latest fetchFn so the effect sees the current version
  // without re-running on every render if the caller forgot useCallback.
  const fetchFnRef = useRef(fetchFn);
  fetchFnRef.current = fetchFn;

  const refetch = useCallback(() => {
    setTick((t) => t + 1);
  }, []);

  useEffect(() => {
    let cancelled = false;

    setLoading(true);
    setError(null);

    fetchFnRef.current()
      .then((result) => {
        if (!cancelled) {
          setData(result);
          setLoading(false);
        }
      })
      .catch((err: unknown) => {
        if (!cancelled) {
          const message =
            err instanceof ApiError
              ? err.message
              : err instanceof Error
                ? err.message
                : 'Unknown error';
          setError(message);
          setLoading(false);
        }
      });

    return () => {
      cancelled = true;
    };
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tick]);

  return { data, loading, error, refetch };
}
