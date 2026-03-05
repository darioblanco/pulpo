import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
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

describe('App', () => {
  it('renders the dashboard page', () => {
    render(<App />);
    expect(screen.getByTestId('dashboard-page')).toBeInTheDocument();
  });
});
