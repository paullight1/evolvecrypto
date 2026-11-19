import { get, post } from '@shared/api/client';
import type { LaunchpadProject, LaunchpadCommitment, CommitRequest } from './types';

export const getProjects = (): Promise<LaunchpadProject[]> =>
  get<{ projects: LaunchpadProject[] } | LaunchpadProject[]>('/launchpad/projects').then((res) =>
    Array.isArray(res) ? res : res.projects,
  );

export const commitToProject = (req: CommitRequest): Promise<{ estimated_tokens: string; committed_usdt: string }> =>
  post<{ estimated_tokens: string; committed_usdt: string }>('/launchpad/commit', req);

export const getMyCommitments = (): Promise<LaunchpadCommitment[]> =>
  get<LaunchpadCommitment[]>('/launchpad/my-commitments');
