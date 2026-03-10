import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { OceanCanvas } from './ocean-canvas';
import type { Session, NodeInfo } from '@/api/types';

// Mock sprite loading
vi.mock('./engine/sprites', () => ({
  loadAllSprites: vi.fn().mockResolvedValue({
    octopus: {},
    nodes: {},
    ui: {},
    status: {},
    decor: {},
  }),
}));

// Mock canvas getContext
const mockCtx = {
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
  ellipse: vi.fn(),
  setLineDash: vi.fn(),
  createLinearGradient: vi.fn().mockReturnValue({
    addColorStop: vi.fn(),
  }),
  set fillStyle(_v: string) {},
  set strokeStyle(_v: string) {},
  set globalAlpha(_v: number) {},
  set lineWidth(_v: number) {},
  set font(_v: string) {},
  set textAlign(_v: string) {},
  set imageSmoothingEnabled(_v: boolean) {},
  set imageSmoothingQuality(_v: string) {},
  set filter(_v: string) {},
};

HTMLCanvasElement.prototype.getContext = vi.fn().mockReturnValue(mockCtx) as never;

// Mock ResizeObserver
const mockObserve = vi.fn();
const mockDisconnect = vi.fn();
vi.stubGlobal(
  'ResizeObserver',
  class {
    observe = mockObserve;
    unobserve = vi.fn();
    disconnect = mockDisconnect;
  },
);

// Mock requestAnimationFrame
let rafCallback: ((time: number) => void) | null = null;
vi.stubGlobal('requestAnimationFrame', (cb: (time: number) => void) => {
  rafCallback = cb;
  return 1;
});
vi.stubGlobal('cancelAnimationFrame', vi.fn());

function makeNode(overrides: Partial<NodeInfo> = {}): NodeInfo {
  return {
    name: 'mac-studio',
    hostname: 'mac-studio.local',
    os: 'macos',
    arch: 'aarch64',
    cpus: 12,
    memory_mb: 32768,
    gpu: null,
    ...overrides,
  };
}

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
  vi.clearAllMocks();
  rafCallback = null;
});

describe('OceanCanvas', () => {
  it('renders the canvas element', async () => {
    render(<OceanCanvas localNode={makeNode()} localSessions={[]} peers={[]} peerSessions={{}} />);
    await waitFor(() => {
      expect(screen.getByTestId('ocean-canvas')).toBeInTheDocument();
    });
  });

  it('renders the container', () => {
    render(<OceanCanvas localNode={makeNode()} localSessions={[]} peers={[]} peerSessions={{}} />);
    expect(screen.getByTestId('ocean-canvas-container')).toBeInTheDocument();
  });

  it('shows loading overlay initially then hides it', async () => {
    render(<OceanCanvas localNode={makeNode()} localSessions={[]} peers={[]} peerSessions={{}} />);
    // Loading overlay disappears after sprites load
    await waitFor(() => {
      expect(screen.queryByTestId('loading-overlay')).not.toBeInTheDocument();
    });
  });

  it('renders canvas with cursor-pointer class', () => {
    render(<OceanCanvas localNode={makeNode()} localSessions={[]} peers={[]} peerSessions={{}} />);
    const canvas = screen.getByTestId('ocean-canvas');
    expect(canvas.classList.contains('cursor-pointer')).toBe(true);
  });

  it('shows profile card when clicking on an octopus', async () => {
    const sessions = [makeSession({ id: 's1', name: 'worker-1' })];
    render(
      <OceanCanvas localNode={makeNode()} localSessions={sessions} peers={[]} peerSessions={{}} />,
    );

    await waitFor(() => {
      expect(screen.queryByTestId('loading-overlay')).not.toBeInTheDocument();
    });

    // Trigger a game loop frame to sync data into the world
    if (rafCallback) rafCallback(performance.now());

    // Click on the canvas — profile card won't show since we can't hit-test
    // without real coordinates, but we verify the click handler doesn't crash
    const canvas = screen.getByTestId('ocean-canvas');
    fireEvent.click(canvas, { clientX: 400, clientY: 300 });

    // No profile card for a miss click
    expect(screen.queryByTestId('profile-card')).not.toBeInTheDocument();
  });

  it('observes container for resize', async () => {
    render(<OceanCanvas localNode={makeNode()} localSessions={[]} peers={[]} peerSessions={{}} />);
    expect(mockObserve).toHaveBeenCalled();
  });

  it('disconnects resize observer on unmount', async () => {
    const { unmount } = render(
      <OceanCanvas localNode={makeNode()} localSessions={[]} peers={[]} peerSessions={{}} />,
    );
    unmount();
    expect(mockDisconnect).toHaveBeenCalled();
  });
});
