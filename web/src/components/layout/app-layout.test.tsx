import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen } from '@testing-library/react';
import { MemoryRouter, Route, Routes } from 'react-router';
import { TooltipProvider } from '@/components/ui/tooltip';
import { ConnectionProvider } from '@/hooks/use-connection';
import { SSEProvider } from '@/hooks/use-sse';
import { setApiConfig } from '@/api/client';
import { AppLayout } from './app-layout';

vi.stubGlobal('localStorage', {
  getItem: () => null,
  setItem: () => {},
  removeItem: () => {},
});

vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ json: () => Promise.resolve([]) }));

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

beforeEach(() => {
  setApiConfig({ getBaseUrl: () => '', getAuthToken: () => '' });
});

describe('AppLayout', () => {
  it('renders outlet content with sidebar', () => {
    render(
      <MemoryRouter>
        <ConnectionProvider>
          <SSEProvider>
            <TooltipProvider>
              <Routes>
                <Route element={<AppLayout />}>
                  <Route index element={<div data-testid="child">Hello</div>} />
                </Route>
              </Routes>
            </TooltipProvider>
          </SSEProvider>
        </ConnectionProvider>
      </MemoryRouter>,
    );
    expect(screen.getByTestId('child')).toBeInTheDocument();
    expect(screen.getByTestId('app-sidebar')).toBeInTheDocument();
  });

  it('shows disconnected banner when SSE is not connected', () => {
    render(
      <MemoryRouter>
        <ConnectionProvider>
          <SSEProvider>
            <TooltipProvider>
              <Routes>
                <Route element={<AppLayout />}>
                  <Route index element={<div>Content</div>} />
                </Route>
              </Routes>
            </TooltipProvider>
          </SSEProvider>
        </ConnectionProvider>
      </MemoryRouter>,
    );
    expect(screen.getByTestId('disconnected-banner')).toBeInTheDocument();
    expect(screen.getByText(/Disconnected from pulpod/)).toBeInTheDocument();
  });
});
