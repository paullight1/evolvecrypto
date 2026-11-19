import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { renderHook, waitFor, act } from '@testing-library/react';
import { useFetch } from './useFetch';
import { ApiError } from '@shared/api/client';

describe('useFetch', () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('starts with loading=true and no data', () => {
    // Use a promise we never resolve so we can inspect the initial state
    const fetchFn = vi.fn(() => new Promise<string[]>(() => {}));
    const { result } = renderHook(() => useFetch(fetchFn));
    expect(result.current.loading).toBe(true);
    expect(result.current.data).toBeNull();
    expect(result.current.error).toBeNull();
  });

  it('sets data and loading=false on success', async () => {
    const data = [{ id: '1' }];
    const fetchFn = vi.fn(() => Promise.resolve(data));
    const { result } = renderHook(() => useFetch(fetchFn));

    await waitFor(() => expect(result.current.loading).toBe(false));

    expect(result.current.data).toEqual(data);
    expect(result.current.error).toBeNull();
  });

  it('calls the fetchFn once on mount', async () => {
    const fetchFn = vi.fn(() => Promise.resolve('ok'));
    const { result } = renderHook(() => useFetch(fetchFn));

    await waitFor(() => expect(result.current.loading).toBe(false));

    expect(fetchFn).toHaveBeenCalledOnce();
  });

  it('sets error and loading=false on generic Error failure', async () => {
    const fetchFn = vi.fn(() => Promise.reject(new Error('network timeout')));
    const { result } = renderHook(() => useFetch(fetchFn));

    await waitFor(() => expect(result.current.loading).toBe(false));

    expect(result.current.error).toBe('network timeout');
    expect(result.current.data).toBeNull();
  });

  it('extracts message from ApiError', async () => {
    const fetchFn = vi.fn(() => Promise.reject(new ApiError(404, 'Not found')));
    const { result } = renderHook(() => useFetch(fetchFn));

    await waitFor(() => expect(result.current.loading).toBe(false));

    expect(result.current.error).toBe('Not found');
  });

  it('sets error to "Unknown error" for non-Error rejection value', async () => {
    const fetchFn = vi.fn(() => Promise.reject('plain string'));
    const { result } = renderHook(() => useFetch(fetchFn));

    await waitFor(() => expect(result.current.loading).toBe(false));

    expect(result.current.error).toBe('Unknown error');
  });

  it('refetch re-triggers the fetch function', async () => {
    let callCount = 0;
    const fetchFn = vi.fn(() => {
      callCount += 1;
      return Promise.resolve(`call-${callCount}`);
    });

    const { result } = renderHook(() => useFetch(fetchFn));
    await waitFor(() => expect(result.current.loading).toBe(false));

    expect(result.current.data).toBe('call-1');

    act(() => {
      result.current.refetch();
    });

    await waitFor(() => expect(result.current.loading).toBe(false));

    expect(result.current.data).toBe('call-2');
    expect(fetchFn).toHaveBeenCalledTimes(2);
  });

  it('refetch resets loading to true before the second fetch resolves', async () => {
    let resolve: (v: string) => void = () => {};
    const fetchFn = vi
      .fn()
      .mockResolvedValueOnce('first')
      .mockImplementationOnce(
        () =>
          new Promise<string>((res) => {
            resolve = res;
          }),
      );

    const { result } = renderHook(() => useFetch(fetchFn));
    await waitFor(() => expect(result.current.loading).toBe(false));

    act(() => {
      result.current.refetch();
    });

    // Loading should go back to true while the second fetch is in flight
    expect(result.current.loading).toBe(true);

    act(() => {
      resolve('second');
    });
    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(result.current.data).toBe('second');
  });

  it('updates data when fetchFn returns different values across refetch calls', async () => {
    const fetchFn = vi
      .fn()
      .mockResolvedValueOnce({ count: 1 })
      .mockResolvedValueOnce({ count: 2 });

    const { result } = renderHook(() => useFetch(fetchFn));
    await waitFor(() => expect(result.current.data).toEqual({ count: 1 }));

    act(() => {
      result.current.refetch();
    });
    await waitFor(() => expect(result.current.data).toEqual({ count: 2 }));
  });
});
