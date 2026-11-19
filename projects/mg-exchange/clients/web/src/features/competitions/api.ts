import { get, post } from '@shared/api/client';
import type { Competition, CompetitionEntry, JoinRequest } from './types';

export const getCompetitions = (): Promise<Competition[]> =>
  get<Competition[]>('/competitions');

export const getLeaderboard = (id: string): Promise<CompetitionEntry[]> =>
  get<CompetitionEntry[]>(`/competitions/${id}/leaderboard`);

export const joinCompetition = (req: JoinRequest): Promise<void> =>
  post<void>(`/competitions/${req.competition_id}/join`, {});
