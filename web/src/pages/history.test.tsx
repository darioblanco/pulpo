import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor, fireEvent } from '@testing-library/react';
import { SidebarProvider } from '@/components/ui/sidebar';
import { TooltipProvider } from '@/components/ui/tooltip';
import { ConnectionProvider } from '@/hooks/use-connection';
import { HistoryPage } from './history';
import * as api from '@/api/client';
import type { Session } from '@/api/types';

vi.mock('@/api/client', () => ({
  getSessions: vi.fn(),
  deleteSession: vi.fn(),
  downloadSessionOutput: vi.fn(),
  resolveBaseUrl: vi.fn().mockReturnValue(''),
  authHeaders: vi.fn().mockReturnValue({}),
  setApiConfig: vi.fn(),
}));

vi.stubGlobal('localStorage', {
  getItem: () => null,
  setItem: () => {},
  removeItem: () => {},
});

const mockGetSessions = vi.mocked(api.getSessions);

beforeEach(() => {
  mockGetSessions.mockReset();
});

function makeSession(overrides: Partial<Session> = {}): Session {
  return {
    id: 'sess-1',
    name: 'my-api',
    status: 'finished',
    command: 'Fix the bug',
    description: null,
    workdir: '/repo',
    metadata: null,
    ink: null,
    intervention_reason: null,
    intervention_at: null,
    last_output_at: null,

    created_at: '2025-01-01T00:00:00Z',
    ...overrides,
  };
}

function renderHistory() {
  return render(
    <ConnectionProvider>
      <TooltipProvider>
        <SidebarProvider>
          <HistoryPage />
        </SidebarProvider>
      </TooltipProvider>
    </ConnectionProvider>,
  );
}

describe('HistoryPage', () => {
  it('renders with loading skeleton', () => {
    mockGetSessions.mockResolvedValue([]);
    renderHistory();
    expect(screen.getByTestId('history-page')).toBeInTheDocument();
    expect(screen.getByTestId('loading-skeleton')).toBeInTheDocument();
  });

  it('shows sessions after loading', async () => {
    mockGetSessions.mockResolvedValue([makeSession()]);
    renderHistory();
    await waitFor(() => {
      expect(screen.getByText('my-api')).toBeInTheDocument();
    });
  });

  it('shows empty message when no sessions', async () => {
    mockGetSessions.mockResolvedValue([]);
    renderHistory();
    await waitFor(() => {
      expect(screen.getByText('No sessions found.')).toBeInTheDocument();
    });
  });

  it('shows error on fetch failure', async () => {
    mockGetSessions.mockRejectedValue(new Error('Network error'));
    renderHistory();
    await waitFor(() => {
      expect(screen.getByText('Failed to load sessions')).toBeInTheDocument();
    });
  });

  it('fetches with default filter (finished,killed)', async () => {
    mockGetSessions.mockResolvedValue([]);
    renderHistory();
    await waitFor(() => {
      expect(mockGetSessions).toHaveBeenCalledWith({
        status: 'finished,killed',
      });
    });
  });

  it('re-fetches when filter changes', async () => {
    mockGetSessions.mockResolvedValue([]);
    renderHistory();
    await waitFor(() => {
      expect(mockGetSessions).toHaveBeenCalled();
    });

    // Click a status filter
    fireEvent.click(screen.getByTestId('status-chip-killed'));
    await waitFor(() => {
      expect(mockGetSessions).toHaveBeenCalledWith({
        search: undefined,
        status: 'killed',
      });
    });
  });

  it('refreshes after session deletion', async () => {
    const deleteSession = vi.mocked(api.deleteSession);
    deleteSession.mockResolvedValue(undefined);
    mockGetSessions.mockResolvedValue([makeSession()]);
    renderHistory();

    await waitFor(() => {
      expect(screen.getByText('my-api')).toBeInTheDocument();
    });

    // Expand and delete
    fireEvent.click(screen.getByTestId('history-item-sess-1'));
    fireEvent.click(screen.getByTestId('delete-sess-1'));

    await waitFor(() => {
      // onRefresh calls fetchSessions(filter) again
      expect(mockGetSessions).toHaveBeenCalledTimes(2);
    });
  });
});
