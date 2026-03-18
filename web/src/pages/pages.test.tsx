import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router';
import { SidebarProvider } from '@/components/ui/sidebar';
import { TooltipProvider } from '@/components/ui/tooltip';
import { ConnectionProvider } from '@/hooks/use-connection';
import { SSEProvider } from '@/hooks/use-sse';
import { setApiConfig } from '@/api/client';
import { HistoryPage } from './history';
import { SettingsPage } from './settings';
import { ConnectPage } from './connect';

vi.stubGlobal('localStorage', {
  getItem: () => null,
  setItem: () => {},
  removeItem: () => {},
});

vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: true, json: () => Promise.resolve([]) }));

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

function wrapWithProviders(ui: React.ReactElement) {
  return render(
    <MemoryRouter>
      <ConnectionProvider>
        <SSEProvider>
          <TooltipProvider>
            <SidebarProvider>{ui}</SidebarProvider>
          </TooltipProvider>
        </SSEProvider>
      </ConnectionProvider>
    </MemoryRouter>,
  );
}

describe('HistoryPage', () => {
  it('renders', () => {
    wrapWithProviders(<HistoryPage />);
    expect(screen.getByTestId('history-page')).toBeInTheDocument();
  });
});

describe('SettingsPage', () => {
  it('renders', () => {
    wrapWithProviders(<SettingsPage />);
    expect(screen.getByTestId('settings-page')).toBeInTheDocument();
  });
});

describe('ConnectPage', () => {
  it('renders', () => {
    render(
      <MemoryRouter>
        <ConnectionProvider>
          <ConnectPage />
        </ConnectionProvider>
      </MemoryRouter>,
    );
    expect(screen.getByTestId('connect-page')).toBeInTheDocument();
  });
});
