import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import { SidebarProvider } from '@/components/ui/sidebar';
import { TooltipProvider } from '@/components/ui/tooltip';
import { ConnectionProvider } from '@/hooks/use-connection';
import { SSEProvider } from '@/hooks/use-sse';
import { MemoryRouter } from 'react-router';
import { OceanPage } from './ocean';
import * as api from '@/api/client';

vi.mock('@/api/client', () => ({
  getPeers: vi.fn(),
  getSessions: vi.fn(),
  getRemoteSessions: vi.fn(),
  resolveBaseUrl: vi.fn().mockReturnValue(''),
  authHeaders: vi.fn().mockReturnValue({}),
  setApiConfig: vi.fn(),
}));

// Mock sprite loading (canvas engine)
vi.mock('@/components/ocean/engine/sprites', () => ({
  loadAllSprites: vi.fn().mockResolvedValue({
    octopus: {},
    nodes: {},
    ui: {},
    status: {},
    decor: {},
    fauna: {},
  }),
  loadBackground: vi.fn().mockResolvedValue({}),
}));

// Mock canvas getContext
HTMLCanvasElement.prototype.getContext = vi.fn().mockReturnValue({
  save: vi.fn(),
  restore: vi.fn(),
  scale: vi.fn(),
  clearRect: vi.fn(),
  fillRect: vi.fn(),
  fillText: vi.fn(),
  drawImage: vi.fn(),
  beginPath: vi.fn(),
  moveTo: vi.fn(),
  lineTo: vi.fn(),
  arc: vi.fn(),
  fill: vi.fn(),
  stroke: vi.fn(),
  createLinearGradient: vi.fn().mockReturnValue({ addColorStop: vi.fn() }),
  set fillStyle(_v: string) {},
  set strokeStyle(_v: string) {},
  set globalAlpha(_v: number) {},
  set lineWidth(_v: number) {},
  set font(_v: string) {},
  set textAlign(_v: string) {},
  set imageSmoothingEnabled(_v: boolean) {},
  set imageSmoothingQuality(_v: string) {},
  set filter(_v: string) {},
}) as never;

vi.stubGlobal(
  'ResizeObserver',
  class {
    observe = vi.fn();
    unobserve = vi.fn();
    disconnect = vi.fn();
  },
);

vi.stubGlobal('requestAnimationFrame', vi.fn().mockReturnValue(1));
vi.stubGlobal('cancelAnimationFrame', vi.fn());

vi.stubGlobal('localStorage', {
  getItem: () => null,
  setItem: () => {},
  removeItem: () => {},
});

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
const mockGetRemoteSessions = vi.mocked(api.getRemoteSessions);

beforeEach(() => {
  mockGetPeers.mockReset();
  mockGetSessions.mockReset();
  mockGetRemoteSessions.mockReset();
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

  it('shows loading skeleton initially', () => {
    renderOcean();
    expect(screen.getByTestId('loading-skeleton')).toBeInTheDocument();
  });

  it('shows the tide pool grid after peers load', async () => {
    renderOcean();
    await waitFor(() => {
      expect(screen.getByTestId('tide-pool-grid')).toBeInTheDocument();
    });
  });

  it('renders a tide pool for the local node', async () => {
    renderOcean();
    await waitFor(() => {
      expect(screen.getByTestId('tide-pool')).toBeInTheDocument();
    });
  });

  it('renders tide pool canvas', async () => {
    renderOcean();
    await waitFor(() => {
      expect(screen.getByTestId('tide-pool-canvas')).toBeInTheDocument();
    });
  });

  it('renders tide pool canvas container', async () => {
    renderOcean();
    await waitFor(() => {
      expect(screen.getByTestId('tide-pool-canvas-container')).toBeInTheDocument();
    });
  });

  it('renders multiple tide pools for peers', async () => {
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
      peers: [
        {
          name: 'linux-box',
          address: '10.0.0.2:7433',
          status: 'online',
          node_info: null,
          session_count: null,
        },
      ],
    });
    mockGetRemoteSessions.mockResolvedValue([]);

    renderOcean();
    await waitFor(() => {
      const pools = screen.getAllByTestId('tide-pool');
      expect(pools).toHaveLength(2);
    });
  });

  it('shows node name in tide pool header', async () => {
    renderOcean();
    await waitFor(() => {
      expect(screen.getByText('mac-studio')).toBeInTheDocument();
    });
  });
});
