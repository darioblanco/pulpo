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

// The home route renders the sessions dashboard directly, which fetches the local
// node info + sessions on mount. Give those calls well-shaped empty responses so
// the smoke test renders cleanly.
vi.mock('@/api/client', async () => {
  const actual = await vi.importActual<typeof import('@/api/client')>('@/api/client');
  return {
    ...actual,
    getNode: vi.fn().mockResolvedValue({ name: 'test-node' }),
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
