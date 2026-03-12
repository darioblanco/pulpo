import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { MemoryRouter } from 'react-router';
import { TidePool } from './tide-pool';
import type { Session } from '@/api/types';
import type { Sprites } from './engine/sprites';

// Mock sprite loading
vi.mock('./engine/sprites', () => ({
  loadBackground: vi.fn().mockResolvedValue({}),
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

function makeSession(overrides: Partial<Session> = {}): Session {
  return {
    id: 'sess-1',
    name: 'api-fix',
    provider: 'claude',
    status: 'active',
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

    created_at: '2026-01-01T00:00:00Z',
    ...overrides,
  };
}

const mockSprites: Sprites = {
  octopus: {},
  nodes: {},
  ui: {},
  status: {},
  decor: {},
  fauna: {},
};

const defaultProps: {
  nodeName: string;
  isLocal: boolean;
  nodeStatus: 'online' | 'offline' | 'unknown';
  sessions: Session[];
  nodeColor: string;
  sprites: Sprites | null;
} = {
  nodeName: 'mac-studio',
  isLocal: true,
  nodeStatus: 'online',
  sessions: [],
  nodeColor: '#f472b6',
  sprites: mockSprites,
};

function renderTidePool(overrides: Partial<typeof defaultProps> = {}) {
  return render(
    <MemoryRouter>
      <TidePool {...defaultProps} {...overrides} />
    </MemoryRouter>,
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  rafCallback = null;
});

describe('TidePool', () => {
  it('renders the tide pool container', () => {
    renderTidePool();
    expect(screen.getByTestId('tide-pool')).toBeInTheDocument();
  });

  it('renders the canvas element', async () => {
    renderTidePool();
    await waitFor(() => {
      expect(screen.getByTestId('tide-pool-canvas')).toBeInTheDocument();
    });
  });

  it('renders the canvas container', () => {
    renderTidePool();
    expect(screen.getByTestId('tide-pool-canvas-container')).toBeInTheDocument();
  });

  it('renders node name in header', () => {
    renderTidePool();
    expect(screen.getByText('mac-studio')).toBeInTheDocument();
  });

  it('shows local indicator for local node', () => {
    renderTidePool({ isLocal: true });
    expect(screen.getByText('(local)')).toBeInTheDocument();
  });

  it('does not show local indicator for peer node', () => {
    renderTidePool({ isLocal: false });
    expect(screen.queryByText('(local)')).not.toBeInTheDocument();
  });

  it('shows session count', () => {
    const sessions = [makeSession({ id: 's1' }), makeSession({ id: 's2' })];
    renderTidePool({ sessions });
    expect(screen.getByText('2 sessions')).toBeInTheDocument();
  });

  it('shows singular session for 1 session', () => {
    renderTidePool({ sessions: [makeSession()] });
    expect(screen.getByText('1 session')).toBeInTheDocument();
  });

  it('shows 0 sessions text', () => {
    renderTidePool({ sessions: [] });
    expect(screen.getByText('0 sessions')).toBeInTheDocument();
  });

  it('renders status dot', () => {
    renderTidePool();
    expect(screen.getByTestId('tide-pool-status-dot')).toBeInTheDocument();
  });

  it('renders header', () => {
    renderTidePool();
    expect(screen.getByTestId('tide-pool-header')).toBeInTheDocument();
  });

  it('renders canvas with cursor-pointer class', () => {
    renderTidePool();
    const canvas = screen.getByTestId('tide-pool-canvas');
    expect(canvas.classList.contains('cursor-pointer')).toBe(true);
  });

  it('handles click without crashing', async () => {
    const sessions = [makeSession({ id: 's1', name: 'worker-1' })];
    renderTidePool({ sessions });

    await waitFor(() => {
      expect(screen.queryByTestId('tide-pool-loading')).not.toBeInTheDocument();
    });

    if (rafCallback) rafCallback(performance.now());

    const canvas = screen.getByTestId('tide-pool-canvas');
    fireEvent.click(canvas, { clientX: 400, clientY: 300 });

    // No profile card for a miss click
    expect(screen.queryByTestId('profile-card')).not.toBeInTheDocument();
  });

  it('observes container for resize', () => {
    renderTidePool();
    expect(mockObserve).toHaveBeenCalled();
  });

  it('disconnects resize observer on unmount', () => {
    const { unmount } = renderTidePool();
    unmount();
    expect(mockDisconnect).toHaveBeenCalled();
  });

  it('shows loading when sprites are null', () => {
    renderTidePool({ sprites: null });
    expect(screen.getByTestId('tide-pool-loading')).toBeInTheDocument();
  });

  it('hides loading after background loads with sprites', async () => {
    renderTidePool();
    await waitFor(() => {
      expect(screen.queryByTestId('tide-pool-loading')).not.toBeInTheDocument();
    });
  });

  it('renders with border styling', () => {
    renderTidePool();
    const container = screen.getByTestId('tide-pool-canvas-container');
    expect(container.classList.contains('border')).toBe(true);
    expect(container.classList.contains('rounded-lg')).toBe(true);
    expect(container.classList.contains('overflow-hidden')).toBe(true);
  });

  it('uses 16:9 aspect ratio', () => {
    renderTidePool();
    const container = screen.getByTestId('tide-pool-canvas-container');
    expect(container.style.aspectRatio).toBe('16 / 9');
  });
});
