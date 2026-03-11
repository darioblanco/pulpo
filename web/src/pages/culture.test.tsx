import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor, fireEvent } from '@testing-library/react';
import { SidebarProvider } from '@/components/ui/sidebar';
import { TooltipProvider } from '@/components/ui/tooltip';
import { ConnectionProvider } from '@/hooks/use-connection';
import { CulturePage } from './culture';
import * as api from '@/api/client';
import type { Culture } from '@/api/types';

vi.mock('@/api/client', () => ({
  listCulture: vi.fn(),
  deleteCulture: vi.fn(),
  pushCulture: vi.fn(),
  resolveBaseUrl: vi.fn().mockReturnValue(''),
  authHeaders: vi.fn().mockReturnValue({}),
  setApiConfig: vi.fn(),
}));

vi.stubGlobal('localStorage', {
  getItem: () => null,
  setItem: () => {},
  removeItem: () => {},
});

const mockListCulture = vi.mocked(api.listCulture);
const mockDeleteCulture = vi.mocked(api.deleteCulture);
const mockPushCulture = vi.mocked(api.pushCulture);

beforeEach(() => {
  mockListCulture.mockReset();
  mockDeleteCulture.mockReset();
  mockPushCulture.mockReset();
});

function makeCulture(overrides: Partial<Culture> = {}): Culture {
  return {
    id: 'k-1',
    session_id: 'sess-1',
    kind: 'summary',
    scope_repo: '/tmp/repo',
    scope_ink: 'coder',
    title: 'Auth bug fix',
    body: 'Fixed the authentication token refresh issue',
    tags: ['claude', 'completed'],
    relevance: 0.7,
    created_at: '2025-01-01T00:00:00Z',
    ...overrides,
  };
}

function renderCulture() {
  return render(
    <ConnectionProvider>
      <TooltipProvider>
        <SidebarProvider>
          <CulturePage />
        </SidebarProvider>
      </TooltipProvider>
    </ConnectionProvider>,
  );
}

describe('CulturePage', () => {
  it('renders with loading skeleton', () => {
    mockListCulture.mockResolvedValue({ culture: [] });
    renderCulture();
    expect(screen.getByTestId('culture-page')).toBeInTheDocument();
    expect(screen.getByTestId('loading-skeleton')).toBeInTheDocument();
  });

  it('shows items after loading', async () => {
    mockListCulture.mockResolvedValue({ culture: [makeCulture()] });
    renderCulture();
    await waitFor(() => {
      expect(screen.getByText('Auth bug fix')).toBeInTheDocument();
    });
  });

  it('shows empty message when no items', async () => {
    mockListCulture.mockResolvedValue({ culture: [] });
    renderCulture();
    await waitFor(() => {
      expect(
        screen.getByText('No culture items yet. Culture is extracted from completed sessions.'),
      ).toBeInTheDocument();
    });
  });

  it('shows error on fetch failure', async () => {
    mockListCulture.mockRejectedValue(new Error('Network error'));
    renderCulture();
    await waitFor(() => {
      expect(screen.getByText('Failed to load culture')).toBeInTheDocument();
    });
  });

  it('deletes an item', async () => {
    mockListCulture.mockResolvedValue({ culture: [makeCulture()] });
    mockDeleteCulture.mockResolvedValue({ deleted: true });
    renderCulture();

    await waitFor(() => {
      expect(screen.getByText('Auth bug fix')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByTestId('delete-culture-btn'));

    await waitFor(() => {
      expect(mockDeleteCulture).toHaveBeenCalledWith('k-1');
    });
  });

  it('renders push button', async () => {
    mockListCulture.mockResolvedValue({ culture: [] });
    renderCulture();
    expect(screen.getByTestId('push-culture-btn')).toBeInTheDocument();
  });

  it('pushes culture to remote', async () => {
    mockListCulture.mockResolvedValue({ culture: [] });
    mockPushCulture.mockResolvedValue({ pushed: true, message: 'pushed to remote' });
    renderCulture();

    await waitFor(() => {
      expect(screen.getByTestId('push-culture-btn')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByTestId('push-culture-btn'));

    await waitFor(() => {
      expect(mockPushCulture).toHaveBeenCalled();
    });
  });

  it('shows failure badge for failure items', async () => {
    mockListCulture.mockResolvedValue({
      culture: [makeCulture({ kind: 'failure', title: 'OOM crash' })],
    });
    renderCulture();
    await waitFor(() => {
      expect(screen.getByText('failure')).toBeInTheDocument();
    });
  });

  it('shows tags on culture cards', async () => {
    mockListCulture.mockResolvedValue({ culture: [makeCulture()] });
    renderCulture();
    await waitFor(() => {
      expect(screen.getByText('claude')).toBeInTheDocument();
      expect(screen.getByText('completed')).toBeInTheDocument();
    });
  });

  it('shows item count', async () => {
    mockListCulture.mockResolvedValue({
      culture: [makeCulture(), makeCulture({ id: 'k-2', title: 'Second item' })],
    });
    renderCulture();
    await waitFor(() => {
      expect(screen.getByText('2 items')).toBeInTheDocument();
    });
  });
});
