import { render, screen, act, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('@shared/api/client', () => ({
  get: vi.fn(),
  post: vi.fn(),
}));

vi.mock('@shared/ui/Toast', () => ({
  toast: { success: vi.fn(), error: vi.fn() },
}));

import SocialFeedPage from './SocialFeedPage';
import { get, post } from '@shared/api/client';

const mockGet = vi.mocked(get);
const mockPost = vi.mocked(post);

interface SocialPost {
  id: string;
  author: string;
  body: string;
  likes: number;
  comments: number;
  created_at: number;
}

const MOCK_POSTS: SocialPost[] = [
  { id: 'post-1', author: 'Trader_Alpha', body: 'BTC looking bullish above 65k support', likes: 12, comments: 3, created_at: 1700000000 },
  { id: 'post-2', author: 'Quant99', body: 'ETH/BTC ratio near key resistance', likes: 8, comments: 1, created_at: 1700100000 },
];

beforeEach(() => {
  vi.clearAllMocks();
  mockGet.mockResolvedValue([]);
  mockPost.mockResolvedValue(undefined);
});

describe('SocialFeedPage', () => {
  it('renders feed with posts after load', async () => {
    mockGet.mockResolvedValue(MOCK_POSTS);
    render(<SocialFeedPage />);

    expect(await screen.findByText('BTC looking bullish above 65k support')).toBeInTheDocument();
    expect(screen.getByText('ETH/BTC ratio near key resistance')).toBeInTheDocument();
    expect(screen.getByText('Trader_Alpha')).toBeInTheDocument();
  });

  it('submitting a post calls POST /social/posts', async () => {
    mockGet.mockResolvedValue(MOCK_POSTS);
    render(<SocialFeedPage />);

    const textarea = screen.getByPlaceholderText('Share your trading insights...');
    await userEvent.type(textarea, 'My new trading insight');

    await act(async () => {
      await userEvent.click(screen.getByRole('button', { name: 'Post' }));
    });

    expect(mockPost).toHaveBeenCalledWith('/social/posts', { body: 'My new trading insight' });
  });

  it('empty post is not submitted', async () => {
    render(<SocialFeedPage />);

    await act(async () => {
      await userEvent.click(screen.getByRole('button', { name: 'Post' }));
    });

    expect(mockPost).not.toHaveBeenCalled();
  });

  it('feed refreshes after post submission', async () => {
    mockGet
      .mockResolvedValueOnce([])   // initial load — empty
      .mockResolvedValueOnce(MOCK_POSTS);  // after post, reload

    render(<SocialFeedPage />);
    // Initially empty, wait for feed to settle
    await waitFor(() => {
      expect(screen.queryByText('Loading feed...')).not.toBeInTheDocument();
    });

    await userEvent.type(screen.getByPlaceholderText('Share your trading insights...'), 'Test insight');
    await act(async () => {
      await userEvent.click(screen.getByRole('button', { name: 'Post' }));
    });

    expect(mockGet).toHaveBeenCalledTimes(2);
    expect(await screen.findByText('Trader_Alpha')).toBeInTheDocument();
  });

  it('like button calls POST /social/posts/:id/like', async () => {
    mockGet.mockResolvedValue(MOCK_POSTS);
    render(<SocialFeedPage />);

    await screen.findByText('Trader_Alpha');

    // Like buttons rendered as "♥ 12" — find the first one
    const likeBtn = screen.getAllByText(/♥/)[0];
    await act(async () => {
      await userEvent.click(likeBtn);
    });

    expect(mockPost).toHaveBeenCalledWith('/social/posts/post-1/like', undefined);
  });
});
