import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router';
import { MobileNav } from './mobile-nav';

function renderMobileNav(initialPath = '/') {
  return render(
    <MemoryRouter initialEntries={[initialPath]}>
      <MobileNav />
    </MemoryRouter>,
  );
}

describe('MobileNav', () => {
  it('renders the nav with all items', () => {
    renderMobileNav();
    expect(screen.getByTestId('mobile-nav')).toBeInTheDocument();
    expect(screen.getByText('Sessions')).toBeInTheDocument();
    expect(screen.getByText('Usage')).toBeInTheDocument();
    expect(screen.getByText('Schedules')).toBeInTheDocument();
    expect(screen.getByText('Settings')).toBeInTheDocument();
  });
});

describe('MobileNav active state', () => {
  it('marks Sessions active at the root path', () => {
    renderMobileNav('/');
    expect(screen.getByRole('link', { name: /Sessions/i }).className).toContain('text-primary');
    expect(screen.getByRole('link', { name: /Usage/i }).className).toContain(
      'text-muted-foreground',
    );
  });

  it('marks Sessions active on a session detail route', () => {
    renderMobileNav('/sessions/abc');
    expect(screen.getByRole('link', { name: /Sessions/i }).className).toContain('text-primary');
    expect(screen.getByRole('link', { name: /Usage/i }).className).toContain(
      'text-muted-foreground',
    );
  });

  it('marks Usage active on the usage route', () => {
    renderMobileNav('/usage');
    expect(screen.getByRole('link', { name: /Usage/i }).className).toContain('text-primary');
    expect(screen.getByRole('link', { name: /Sessions/i }).className).toContain(
      'text-muted-foreground',
    );
  });
});
