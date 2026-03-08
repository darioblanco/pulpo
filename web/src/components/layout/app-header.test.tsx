import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import { SidebarProvider } from '@/components/ui/sidebar';
import { TooltipProvider } from '@/components/ui/tooltip';
import { AppHeader } from './app-header';

vi.stubGlobal('localStorage', {
  getItem: () => null,
  setItem: () => {},
  removeItem: () => {},
});

describe('AppHeader', () => {
  it('renders the title', () => {
    render(
      <TooltipProvider>
        <SidebarProvider>
          <AppHeader title="Dashboard" />
        </SidebarProvider>
      </TooltipProvider>,
    );
    expect(screen.getByTestId('app-header')).toBeInTheDocument();
    expect(screen.getByText('Dashboard')).toBeInTheDocument();
  });

  it('renders children in header', () => {
    render(
      <TooltipProvider>
        <SidebarProvider>
          <AppHeader title="Test">
            <button>Action</button>
          </AppHeader>
        </SidebarProvider>
      </TooltipProvider>,
    );
    expect(screen.getByText('Action')).toBeInTheDocument();
  });

  it('renders sidebar trigger', () => {
    render(
      <TooltipProvider>
        <SidebarProvider>
          <AppHeader title="Test" />
        </SidebarProvider>
      </TooltipProvider>,
    );
    // SidebarTrigger renders buttons (desktop + mobile)
    const buttons = screen.getAllByRole('button');
    expect(buttons.length).toBeGreaterThanOrEqual(1);
  });
});
