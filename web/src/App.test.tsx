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

// The home route renders the sessions dashboard directly, which fetches peers +
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

describe('App', () => {
  it('renders the sessions dashboard at the home route', async () => {
    render(<App />);
    await waitFor(() => {
      expect(screen.getByTestId('dashboard-page')).toBeInTheDocument();
    });
  });
});
