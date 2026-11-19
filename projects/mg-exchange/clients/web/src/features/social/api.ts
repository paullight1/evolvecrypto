import { get, post } from '@shared/api/client';
import type { SocialPost, SocialComment, TraderProfile } from './types';

export const getFeed = (): Promise<SocialPost[]> =>
  get<SocialPost[]>('/social/feed');

export const createPost = (body: string): Promise<{ id: string }> =>
  post<{ id: string }>('/social/posts', { body });

export const likePost = (postId: string): Promise<void> =>
  post<void>(`/social/posts/${postId}/like`, undefined);

export const getComments = (postId: string): Promise<SocialComment[]> =>
  get<SocialComment[]>(`/social/posts/${postId}/comments`);

export const addComment = (postId: string, body: string): Promise<{ id: string }> =>
  post<{ id: string }>(`/social/posts/${postId}/comments`, { body });

export const getMyProfile = (): Promise<TraderProfile> =>
  get<TraderProfile>('/social/profile/me');

export const getTraderProfile = (userId: string): Promise<TraderProfile> =>
  get<TraderProfile>(`/social/profile/${userId}`);

export const followTrader = (userId: string): Promise<void> =>
  post<void>(`/social/follow/${userId}`, undefined);

export const unfollowTrader = (userId: string): Promise<void> =>
  post<void>(`/social/unfollow/${userId}`, undefined);
