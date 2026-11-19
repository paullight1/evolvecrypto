import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { getProjects, commitToProject, getMyCommitments } from './api';

function mockOkFetch(body: unknown, status = 200): void {
  vi.stubGlobal(
    'fetch',
    vi.fn().mockResolvedValue({
      ok: true,
      status,
      json: () => Promise.resolve(body),
    }),
  );
}

function lastFetchArgs(): [string, RequestInit] {
  return (vi.mocked(fetch).mock.calls[0] as unknown) as [string, RequestInit];
}

describe('getProjects', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/launchpad/projects', async () => {
    mockOkFetch([]);
    await getProjects();
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/launchpad/projects');
    expect(init.method).toBe('GET');
  });

  it('returns projects from server', async () => {
    const projects = [{ project_id: 'p1', name: 'NewCoin', token_price: '0.01' }];
    mockOkFetch(projects);
    const result = await getProjects();
    expect(result).toEqual(projects);
  });
});

describe('commitToProject', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends POST to /api/v1/launchpad/commit', async () => {
    mockOkFetch({ estimated_tokens: '1000', committed_usdt: '10' });
    await commitToProject({ project_id: 'p1', amount_usdt: '10', currency: 'USDT' });
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/launchpad/commit');
    expect(init.method).toBe('POST');
  });

  it('sends commit request as JSON body', async () => {
    mockOkFetch({ estimated_tokens: '5000', committed_usdt: '50' });
    const req = { project_id: 'p2', amount_usdt: '50', currency: 'USDT' };
    await commitToProject(req);
    const [, init] = lastFetchArgs();
    expect(JSON.parse(init.body as string)).toEqual(req);
  });

  it('returns estimated tokens and committed amount', async () => {
    const resp = { estimated_tokens: '10000', committed_usdt: '100' };
    mockOkFetch(resp);
    const result = await commitToProject({ project_id: 'p3', amount_usdt: '100', currency: 'USDT' });
    expect(result).toEqual(resp);
  });
});

describe('getMyCommitments', () => {
  beforeEach(() => { sessionStorage.clear(); });
  afterEach(() => { vi.restoreAllMocks(); });

  it('sends GET to /api/v1/launchpad/my-commitments', async () => {
    mockOkFetch([]);
    await getMyCommitments();
    const [url, init] = lastFetchArgs();
    expect(url).toBe('/api/v1/launchpad/my-commitments');
    expect(init.method).toBe('GET');
  });

  it('returns user commitments from server', async () => {
    const commitments = [{ project_id: 'p1', committed_usdt: '100', tokens_allocated: '10000' }];
    mockOkFetch(commitments);
    const result = await getMyCommitments();
    expect(result).toEqual(commitments);
  });
});
