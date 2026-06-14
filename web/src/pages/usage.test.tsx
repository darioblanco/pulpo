import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import { MemoryRouter } from 'react-router';
import { toast } from 'sonner';
import { UsagePage } from './usage';
import * as api from '@/api/client';
import type { UsageProjectionResponse } from '@/api/types';

vi.mock('sonner', () => ({
  toast: { error: vi.fn(), success: vi.fn() },
}));

vi.mock('@/api/client', () => ({
  getUsageProjection: vi.fn(),
  resolveBaseUrl: vi.fn().mockReturnValue(''),
  authHeaders: vi.fn().mockReturnValue({}),
  setApiConfig: vi.fn(),
}));

vi.mock('@/components/layout/app-header', () => ({
  AppHeader: ({ title }: { title: string }) => <div data-testid="mock-app-header">{title}</div>,
}));

function sample(): UsageProjectionResponse {
  return {
    node_name: 'mac-mini',
    generated_at: '2026-06-13T12:00:00Z',
    sessions: [
      {
        session_id: 'id-1',
        session_name: 'fix-auth',
        usage_source: 'claude-jsonl',
        auth_provider: 'claude.ai',
        auth_plan: 'max',
        auth_email: 'a@x.com',
        pool: 'subscription',
        total_tokens: 1_234_000,
        cost_usd: 2.5,
        elapsed_secs: 3600,
        cost_per_hour: 2.5,
        tokens_per_hour: 1_234_000,
        quota_used_percent: null,
        quota_resets_at: null,
        allowance_tokens: null,
        allowance_used_percent: null,
        secs_to_allowance: null,
      },
      {
        session_id: 'id-2',
        session_name: 'codex-refactor',
        usage_source: 'codex-jsonl',
        auth_provider: 'openai',
        auth_plan: null,
        auth_email: 'b@y.com',
        pool: 'headless',
        total_tokens: 50_000,
        cost_usd: null,
        elapsed_secs: 1800,
        cost_per_hour: null,
        tokens_per_hour: 100_000,
        quota_used_percent: 42,
        quota_resets_at: 1_775_073_678,
        allowance_tokens: null,
        allowance_used_percent: null,
        secs_to_allowance: null,
      },
    ],
    accounts: [
      {
        provider: 'claude.ai',
        plan: 'max',
        email: 'a@x.com',
        pool: 'subscription',
        session_count: 1,
        total_tokens: 1_234_000,
        total_cost_usd: 2.5,
        cost_per_hour: 2.5,
        max_quota_used_percent: null,
        cost_is_exact: true,
      },
      {
        provider: 'openai',
        plan: null,
        email: 'b@y.com',
        pool: 'headless',
        session_count: 1,
        total_tokens: 50_000,
        total_cost_usd: null,
        cost_per_hour: null,
        max_quota_used_percent: 42,
        cost_is_exact: false,
      },
    ],
  };
}

function renderPage() {
  return render(
    <MemoryRouter>
      <UsagePage />
    </MemoryRouter>,
  );
}

describe('UsagePage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders sessions and account rollups', async () => {
    vi.mocked(api.getUsageProjection).mockResolvedValue(sample());
    renderPage();

    await waitFor(() => expect(screen.getByTestId('usage-table')).toBeInTheDocument());
    expect(screen.getByText('fix-auth')).toBeInTheDocument();
    expect(screen.getByText('codex-refactor')).toBeInTheDocument();
    // exact Codex quota vs missing-Claude quota
    expect(screen.getByText('42%')).toBeInTheDocument();
    // account cards with cost + pool
    expect(screen.getAllByText('$2.50').length).toBeGreaterThan(0);
    expect(screen.getAllByText('subscription').length).toBeGreaterThan(0);
    expect(screen.getAllByText('headless').length).toBeGreaterThan(0);
    // token compaction
    expect(screen.getAllByText('1.2M').length).toBeGreaterThan(0);
  });

  it('marks scraped (estimated) cost with a ~ and exact cost without', async () => {
    const data = sample();
    data.sessions[0].usage_source = null; // fix-auth becomes scraped → estimated
    data.accounts[0].cost_is_exact = false;
    vi.mocked(api.getUsageProjection).mockResolvedValue(data);
    renderPage();

    await waitFor(() => expect(screen.getByTestId('usage-table')).toBeInTheDocument());
    // Both the scraped session row and its account card show the estimate marker.
    expect(screen.getAllByText('~$2.50').length).toBeGreaterThan(0);
  });

  it('shows empty state when no sessions', async () => {
    vi.mocked(api.getUsageProjection).mockResolvedValue({
      node_name: 'n',
      generated_at: 't',
      sessions: [],
      accounts: [],
    });
    renderPage();
    await waitFor(() => expect(screen.getByTestId('usage-empty')).toBeInTheDocument());
  });

  it('shows an error toast when the fetch fails', async () => {
    vi.mocked(api.getUsageProjection).mockRejectedValue(new Error('boom'));
    renderPage();
    await waitFor(() => expect(toast.error).toHaveBeenCalledWith('Failed to load usage'));
  });

  it('shows a loading skeleton first', () => {
    vi.mocked(api.getUsageProjection).mockReturnValue(new Promise(() => {}));
    renderPage();
    expect(screen.getByTestId('usage-loading')).toBeInTheDocument();
  });
});
