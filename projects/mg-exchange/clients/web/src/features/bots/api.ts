import { get, post } from '@shared/api/client';
import type { Bot, CreateGridBotRequest, CreateDcaBotRequest } from './types';

export const getMyBots = (): Promise<Bot[]> =>
  get<Bot[]>('/bots');

export const createGridBot = (req: CreateGridBotRequest): Promise<{ bot_id: string }> =>
  post<{ bot_id: string }>('/bots/grid', req);

export const createDcaBot = (req: CreateDcaBotRequest): Promise<{ bot_id: string }> =>
  post<{ bot_id: string }>('/bots/dca', req);

export const pauseBot = (botId: string): Promise<void> =>
  post<void>(`/bots/${botId}/pause`);

export const stopBot = (botId: string): Promise<void> =>
  post<void>(`/bots/${botId}/stop`);
