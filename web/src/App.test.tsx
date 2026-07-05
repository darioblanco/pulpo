import { describe, it, expect, vi } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import { App } from './App';

// Mock EventSource
vi.stubGlobal(
  'EventSource',
  class {
    onopen: (() => void) | null = null;
    onerror: (() => void) | null = null;
    addEventListener() {}
    close() {}
  },
);

// Mock localStorage
vi.stubGlobal('localStorage', {
  getItem: () => null,
  setItem: () => {},
  removeItem: () => {},
});

// Mock fetch
vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ json: () => Promise.resolve([]) }));

// The home route now redirects to the sessions dashboard, which fetches peers +
// sessions on mount. Give those calls well-shaped empty responses so the smoke test
// renders cleanly (the raw fetch mock above returns `[]` for everything, which would
// make `resp.peers` undefined).
vi.mock('@/api/client', async () => {
  const actual = await vi.importActual<typeof import('@/api/client')>('@/api/client');
  return {
    ...actual,
    getPeers: vi.fn().mockResolvedValue({ peers: [] }),
    getSessions: vi.fn().mockResolvedValue([]),
  };
});

// Mock canvas for ocean page
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

// Mock sprite loading
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

describe('App', () => {
  it('redirects home to the sessions dashboard (Ocean is no longer the default)', async () => {
    render(<App />);
    await waitFor(() => {
      expect(screen.getByTestId('dashboard-page')).toBeInTheDocument();
    });
  });
});
