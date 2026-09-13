import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import { MemoryRouter } from 'react-router';
import { toast } from 'sonner';
import { UsagePage } from './usage';
import * as api from '@/api/client';
import type { UsageSessionsResponse } from '@/api/types';

vi.mock('sonner', () => ({
  toast: { error: vi.fn(), success: vi.fn() },
}));

vi.mock('@/api/client', () => ({
  getUsageSessions: vi.fn(),
  resolveBaseUrl: vi.fn().mockReturnValue(''),
  authHeaders: vi.fn().mockReturnValue({}),
  setApiConfig: vi.fn(),
}));

vi.mock('@/components/layout/app-header', () => ({
  AppHeader: ({ title }: { title: string }) => <div data-testid="mock-app-header">{title}</div>,
}));

function sample(): UsageSessionsResponse {
  return {
    node_name: 'mac-mini',
    generated_at: '2026-06-13T12:00:00Z',
    sessions: [
      {
        session_id: 'id-1',
        session_name: 'fix-auth',
        workdir: '/repos/api',
        usage_source: 'claude-jsonl',
        total_tokens: 1_234_000,
        cost_usd: 2.5,
      },
      {
        session_id: 'id-2',
        session_name: 'codex-refactor',
        workdir: '/repos/web',
        usage_source: 'codex-jsonl',
        total_tokens: 50_000,
        cost_usd: null,
      },
    ],
    repos: [
      {
        label: '/repos/api',
        session_count: 1,
        total_tokens: 1_234_000,
        total_cost_usd: 2.5,
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

  it('renders exact per-session usage', async () => {
    vi.mocked(api.getUsageSessions).mockResolvedValue(sample());
    renderPage();

    await waitFor(() => expect(screen.getByTestId('usage-table')).toBeInTheDocument());
    expect(screen.getByText('fix-auth')).toBeInTheDocument();
    expect(screen.getByText('codex-refactor')).toBeInTheDocument();
    // source suffix stripped
    expect(screen.getAllByText('claude').length).toBeGreaterThan(0);
    expect(screen.getAllByText('codex').length).toBeGreaterThan(0);
    // exact cost, no scraped/estimate marker anywhere
    expect(screen.getAllByText('$2.50').length).toBeGreaterThan(0);
    expect(screen.queryByText(/~\$/)).not.toBeInTheDocument();
    // token compaction
    expect(screen.getAllByText('1.2M').length).toBeGreaterThan(0);
    // total spend card
    expect(screen.getByTestId('usage-total-card')).toBeInTheDocument();
  });

  it('renders per-repo cost rollups', async () => {
    vi.mocked(api.getUsageSessions).mockResolvedValue(sample());
    renderPage();

    await waitFor(() => expect(screen.getByTestId('dimension-By repo')).toBeInTheDocument());
    expect(screen.getByText('/repos/api')).toBeInTheDocument();
  });

  it('shows a dash for sessions with no recorded usage source', async () => {
    const data = sample();
    data.sessions[1].usage_source = null;
    vi.mocked(api.getUsageSessions).mockResolvedValue(data);
    renderPage();

    await waitFor(() => expect(screen.getByTestId('usage-table')).toBeInTheDocument());
    const row = screen.getByTestId('usage-row-codex-refactor');
    expect(row).toHaveTextContent('—');
  });

  it('shows empty state when no sessions', async () => {
    vi.mocked(api.getUsageSessions).mockResolvedValue({
      node_name: 'n',
      generated_at: 't',
      sessions: [],
      repos: [],
    });
    renderPage();
    await waitFor(() => expect(screen.getByTestId('usage-empty')).toBeInTheDocument());
  });

  it('shows an error toast when the fetch fails', async () => {
    vi.mocked(api.getUsageSessions).mockRejectedValue(new Error('boom'));
    renderPage();
    await waitFor(() => expect(toast.error).toHaveBeenCalledWith('Failed to load usage'));
  });

  it('shows a loading skeleton first', () => {
    vi.mocked(api.getUsageSessions).mockReturnValue(new Promise(() => {}));
    renderPage();
    expect(screen.getByTestId('usage-loading')).toBeInTheDocument();
  });
});
