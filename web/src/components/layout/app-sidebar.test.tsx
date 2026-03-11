import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router';
import { TooltipProvider } from '@/components/ui/tooltip';
import { SidebarProvider } from '@/components/ui/sidebar';
import { ConnectionProvider } from '@/hooks/use-connection';
import { SSEProvider } from '@/hooks/use-sse';
import { setApiConfig } from '@/api/client';
import { AppSidebar } from './app-sidebar';

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

function renderSidebar() {
  return render(
    <MemoryRouter>
      <ConnectionProvider>
        <SSEProvider>
          <TooltipProvider>
            <SidebarProvider defaultOpen>
              <AppSidebar />
            </SidebarProvider>
          </TooltipProvider>
        </SSEProvider>
      </ConnectionProvider>
    </MemoryRouter>,
  );
}

describe('AppSidebar', () => {
  it('renders the sidebar with nav items', () => {
    renderSidebar();
    expect(screen.getByTestId('app-sidebar')).toBeInTheDocument();
    expect(screen.getByText('Dashboard')).toBeInTheDocument();
    expect(screen.getByText('History')).toBeInTheDocument();
    expect(screen.getByText('Culture')).toBeInTheDocument();
    expect(screen.getByText('Ocean')).toBeInTheDocument();
    expect(screen.getByText('Settings')).toBeInTheDocument();
  });

  it('renders pulpo branding', () => {
    renderSidebar();
    // Sidebar renders both desktop and mobile variants
    expect(screen.getAllByText('pulpo').length).toBeGreaterThanOrEqual(1);
  });

  it('shows connection status dot', () => {
    renderSidebar();
    const dots = screen.getAllByTestId('connection-dot');
    expect(dots.length).toBeGreaterThanOrEqual(1);
    // Not connected by default — should show dead color
    expect(dots[0].className).toContain('bg-status-dead');
  });
});
