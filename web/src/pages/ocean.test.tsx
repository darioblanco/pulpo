import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import { SidebarProvider } from '@/components/ui/sidebar';
import { TooltipProvider } from '@/components/ui/tooltip';
import { ConnectionProvider } from '@/hooks/use-connection';
import { SSEProvider } from '@/hooks/use-sse';
import { MemoryRouter } from 'react-router';
import { OceanPage } from './ocean';
import * as api from '@/api/client';
import type { Session } from '@/api/types';

vi.mock('@/api/client', () => ({
  getPeers: vi.fn(),
  getSessions: vi.fn(),
  getRemoteSessions: vi.fn(),
  resolveBaseUrl: vi.fn().mockReturnValue(''),
  authHeaders: vi.fn().mockReturnValue({}),
  setApiConfig: vi.fn(),
}));

vi.stubGlobal('localStorage', {
  getItem: () => null,
  setItem: () => {},
  removeItem: () => {},
});

// Stub EventSource for SSEProvider
vi.stubGlobal(
  'EventSource',
  class {
    onopen: (() => void) | null = null;
    onerror: (() => void) | null = null;
    addEventListener() {}
    close() {}
  },
);

const mockGetPeers = vi.mocked(api.getPeers);
const mockGetSessions = vi.mocked(api.getSessions);

function makeSession(overrides: Partial<Session> = {}): Session {
  return {
    id: 'sess-1',
    name: 'api-fix',
    provider: 'claude',
    status: 'running',
    prompt: 'Fix the auth bug',
    mode: 'autonomous',
    workdir: '/tmp/repo',
    guard_config: null,
    model: null,
    allowed_tools: null,
    system_prompt: null,
    metadata: null,
    ink: null,
    max_turns: null,
    max_budget_usd: null,
    output_format: null,
    intervention_reason: null,
    intervention_at: null,
    last_output_at: null,
    waiting_for_input: false,
    created_at: '2026-01-01T00:00:00Z',
    ...overrides,
  };
}

beforeEach(() => {
  mockGetPeers.mockReset();
  mockGetSessions.mockReset();
  mockGetPeers.mockResolvedValue({
    local: {
      name: 'mac-studio',
      hostname: 'mac-studio.local',
      os: 'macos',
      arch: 'aarch64',
      cpus: 12,
      memory_mb: 32768,
      gpu: null,
    },
    peers: [],
  });
  mockGetSessions.mockResolvedValue([]);
});

function renderOcean() {
  return render(
    <MemoryRouter>
      <ConnectionProvider>
        <SSEProvider>
          <TooltipProvider>
            <SidebarProvider>
              <OceanPage />
            </SidebarProvider>
          </TooltipProvider>
        </SSEProvider>
      </ConnectionProvider>
    </MemoryRouter>,
  );
}

describe('OceanPage', () => {
  it('renders the ocean page', async () => {
    renderOcean();
    await waitFor(() => {
      expect(screen.getByTestId('ocean-page')).toBeInTheDocument();
    });
  });

  it('shows the ocean canvas after loading', async () => {
    mockGetSessions.mockResolvedValue([makeSession()]);
    renderOcean();
    await waitFor(() => {
      expect(screen.getByTestId('ocean-canvas')).toBeInTheDocument();
    });
  });

  it('renders octopuses for sessions', async () => {
    mockGetSessions.mockResolvedValue([
      makeSession({ name: 'worker-a' }),
      makeSession({ name: 'worker-b', id: 's2' }),
    ]);
    renderOcean();
    await waitFor(() => {
      expect(screen.getByTestId('octopus-worker-a')).toBeInTheDocument();
      expect(screen.getByTestId('octopus-worker-b')).toBeInTheDocument();
    });
  });

  it('shows node name on island', async () => {
    renderOcean();
    await waitFor(() => {
      expect(screen.getByText('mac-studio')).toBeInTheDocument();
    });
  });

  it('shows empty ocean when no sessions', async () => {
    renderOcean();
    await waitFor(() => {
      expect(screen.getByText(/no active sessions/i)).toBeInTheDocument();
    });
  });
});
