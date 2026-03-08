import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor, fireEvent } from '@testing-library/react';
import { SidebarProvider } from '@/components/ui/sidebar';
import { TooltipProvider } from '@/components/ui/tooltip';
import { ConnectionProvider } from '@/hooks/use-connection';
import { KnowledgePage } from './knowledge';
import * as api from '@/api/client';
import type { Knowledge } from '@/api/types';

vi.mock('@/api/client', () => ({
  listKnowledge: vi.fn(),
  deleteKnowledge: vi.fn(),
  pushKnowledge: vi.fn(),
  resolveBaseUrl: vi.fn().mockReturnValue(''),
  authHeaders: vi.fn().mockReturnValue({}),
  setApiConfig: vi.fn(),
}));

vi.stubGlobal('localStorage', {
  getItem: () => null,
  setItem: () => {},
  removeItem: () => {},
});

const mockListKnowledge = vi.mocked(api.listKnowledge);
const mockDeleteKnowledge = vi.mocked(api.deleteKnowledge);
const mockPushKnowledge = vi.mocked(api.pushKnowledge);

beforeEach(() => {
  mockListKnowledge.mockReset();
  mockDeleteKnowledge.mockReset();
  mockPushKnowledge.mockReset();
});

function makeKnowledge(overrides: Partial<Knowledge> = {}): Knowledge {
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

function renderKnowledge() {
  return render(
    <ConnectionProvider>
      <TooltipProvider>
        <SidebarProvider>
          <KnowledgePage />
        </SidebarProvider>
      </TooltipProvider>
    </ConnectionProvider>,
  );
}

describe('KnowledgePage', () => {
  it('renders with loading skeleton', () => {
    mockListKnowledge.mockResolvedValue({ knowledge: [] });
    renderKnowledge();
    expect(screen.getByTestId('knowledge-page')).toBeInTheDocument();
    expect(screen.getByTestId('loading-skeleton')).toBeInTheDocument();
  });

  it('shows items after loading', async () => {
    mockListKnowledge.mockResolvedValue({ knowledge: [makeKnowledge()] });
    renderKnowledge();
    await waitFor(() => {
      expect(screen.getByText('Auth bug fix')).toBeInTheDocument();
    });
  });

  it('shows empty message when no items', async () => {
    mockListKnowledge.mockResolvedValue({ knowledge: [] });
    renderKnowledge();
    await waitFor(() => {
      expect(
        screen.getByText('No knowledge items yet. Knowledge is extracted from completed sessions.'),
      ).toBeInTheDocument();
    });
  });

  it('shows error on fetch failure', async () => {
    mockListKnowledge.mockRejectedValue(new Error('Network error'));
    renderKnowledge();
    await waitFor(() => {
      expect(screen.getByText('Failed to load knowledge')).toBeInTheDocument();
    });
  });

  it('deletes an item', async () => {
    mockListKnowledge.mockResolvedValue({ knowledge: [makeKnowledge()] });
    mockDeleteKnowledge.mockResolvedValue({ deleted: true });
    renderKnowledge();

    await waitFor(() => {
      expect(screen.getByText('Auth bug fix')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByTestId('delete-knowledge-btn'));

    await waitFor(() => {
      expect(mockDeleteKnowledge).toHaveBeenCalledWith('k-1');
    });
  });

  it('renders push button', async () => {
    mockListKnowledge.mockResolvedValue({ knowledge: [] });
    renderKnowledge();
    expect(screen.getByTestId('push-knowledge-btn')).toBeInTheDocument();
  });

  it('pushes knowledge to remote', async () => {
    mockListKnowledge.mockResolvedValue({ knowledge: [] });
    mockPushKnowledge.mockResolvedValue({ pushed: true, message: 'pushed to remote' });
    renderKnowledge();

    await waitFor(() => {
      expect(screen.getByTestId('push-knowledge-btn')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByTestId('push-knowledge-btn'));

    await waitFor(() => {
      expect(mockPushKnowledge).toHaveBeenCalled();
    });
  });

  it('shows failure badge for failure items', async () => {
    mockListKnowledge.mockResolvedValue({
      knowledge: [makeKnowledge({ kind: 'failure', title: 'OOM crash' })],
    });
    renderKnowledge();
    await waitFor(() => {
      expect(screen.getByText('failure')).toBeInTheDocument();
    });
  });

  it('shows tags on knowledge cards', async () => {
    mockListKnowledge.mockResolvedValue({ knowledge: [makeKnowledge()] });
    renderKnowledge();
    await waitFor(() => {
      expect(screen.getByText('claude')).toBeInTheDocument();
      expect(screen.getByText('completed')).toBeInTheDocument();
    });
  });

  it('shows item count', async () => {
    mockListKnowledge.mockResolvedValue({
      knowledge: [makeKnowledge(), makeKnowledge({ id: 'k-2', title: 'Second item' })],
    });
    renderKnowledge();
    await waitFor(() => {
      expect(screen.getByText('2 items')).toBeInTheDocument();
    });
  });
});
