import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router';
import { SidebarProvider } from '@/components/ui/sidebar';
import { TooltipProvider } from '@/components/ui/tooltip';
import { ConnectionProvider } from '@/hooks/use-connection';
import { SSEProvider } from '@/hooks/use-sse';
import { WorktreesPage } from './worktrees';
import type { Session } from '@/api/types';

vi.mock('@/api/client', () => ({
  resolveBaseUrl: vi.fn().mockReturnValue(''),
  authHeaders: vi.fn().mockReturnValue({}),
  setApiConfig: vi.fn(),
}));

class MockEventSource {
  url: string;
  onopen: (() => void) | null = null;
  onerror: (() => void) | null = null;
  listeners: Record<string, ((e: { data: string }) => void)[]> = {};

  constructor(url: string) {
    this.url = url;
  }

  addEventListener(type: string, handler: (e: { data: string }) => void) {
    if (!this.listeners[type]) this.listeners[type] = [];
    this.listeners[type].push(handler);
  }

  close() {}
}

vi.stubGlobal('EventSource', MockEventSource);

vi.stubGlobal('localStorage', {
  getItem: () => null,
  setItem: () => {},
  removeItem: () => {},
});

function makeSession(overrides: Partial<Session> = {}): Session {
  return {
    id: 'sess-1',
    name: 'fix-auth',
    status: 'active',
    command: 'claude -p "fix auth"',
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

function renderWorktrees(sessions: Session[] = []) {
  // Mock fetch for SSE hydration
  vi.stubGlobal('fetch', () =>
    Promise.resolve({
      ok: true,
      json: () => Promise.resolve(sessions),
    }),
  );

  return render(
    <MemoryRouter>
      <ConnectionProvider>
        <SSEProvider>
          <TooltipProvider>
            <SidebarProvider>
              <WorktreesPage />
            </SidebarProvider>
          </TooltipProvider>
        </SSEProvider>
      </ConnectionProvider>
    </MemoryRouter>,
  );
}

describe('WorktreesPage', () => {
  it('renders the page', () => {
    renderWorktrees();
    expect(screen.getByTestId('worktrees-page')).toBeInTheDocument();
  });

  it('shows empty state when no worktree sessions', () => {
    renderWorktrees([]);
    expect(screen.getByTestId('empty-state')).toBeInTheDocument();
    expect(screen.getByText('No worktree sessions')).toBeInTheDocument();
  });

  it('shows empty state when sessions exist but none have worktrees', () => {
    renderWorktrees([makeSession()]);
    expect(screen.getByTestId('empty-state')).toBeInTheDocument();
  });
});
