import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { MemoryRouter } from 'react-router';
import { ProfileCard } from './profile-card';
import type { OctopusEntity } from './engine/world';

function makeOctopus(overrides: Partial<OctopusEntity> = {}): OctopusEntity {
  return {
    sessionId: 'sess-1',
    name: 'worker-alpha',
    status: 'running',
    provider: 'claude',
    ink: null,
    waitingForInput: false,
    nodeName: 'mac-studio',
    x: 100,
    y: 100,
    homeX: 100,
    homeY: 100,
    vx: 0,
    vy: 0,
    animFrame: 0,
    animTimer: 0,
    isSwimming: false,
    wanderTimer: 2,
    wanderTargetX: 100,
    wanderTargetY: 100,
    ...overrides,
  };
}

describe('ProfileCard', () => {
  it('renders octopus name', () => {
    render(
      <MemoryRouter>
        <ProfileCard octopus={makeOctopus()} screenX={400} screenY={300} onClose={vi.fn()} />
      </MemoryRouter>,
    );
    expect(screen.getByText('worker-alpha')).toBeInTheDocument();
  });

  it('renders status', () => {
    render(
      <MemoryRouter>
        <ProfileCard
          octopus={makeOctopus({ status: 'completed' })}
          screenX={400}
          screenY={300}
          onClose={vi.fn()}
        />
      </MemoryRouter>,
    );
    expect(screen.getByText('completed')).toBeInTheDocument();
  });

  it('renders provider', () => {
    render(
      <MemoryRouter>
        <ProfileCard octopus={makeOctopus()} screenX={400} screenY={300} onClose={vi.fn()} />
      </MemoryRouter>,
    );
    expect(screen.getAllByText('claude').length).toBeGreaterThan(0);
  });

  it('renders ink when present', () => {
    render(
      <MemoryRouter>
        <ProfileCard
          octopus={makeOctopus({ ink: 'reviewer' })}
          screenX={400}
          screenY={300}
          onClose={vi.fn()}
        />
      </MemoryRouter>,
    );
    expect(screen.getByText('reviewer')).toBeInTheDocument();
  });

  it('does not render ink when null', () => {
    render(
      <MemoryRouter>
        <ProfileCard
          octopus={makeOctopus({ ink: null })}
          screenX={400}
          screenY={300}
          onClose={vi.fn()}
        />
      </MemoryRouter>,
    );
    expect(screen.queryByText('Ink:')).not.toBeInTheDocument();
  });

  it('shows waiting indicator when waiting for input', () => {
    render(
      <MemoryRouter>
        <ProfileCard
          octopus={makeOctopus({ waitingForInput: true })}
          screenX={400}
          screenY={300}
          onClose={vi.fn()}
        />
      </MemoryRouter>,
    );
    expect(screen.getByText('awaiting input')).toBeInTheDocument();
  });

  it('renders node name', () => {
    render(
      <MemoryRouter>
        <ProfileCard octopus={makeOctopus()} screenX={400} screenY={300} onClose={vi.fn()} />
      </MemoryRouter>,
    );
    expect(screen.getByText('mac-studio')).toBeInTheDocument();
  });

  it('renders View Logs link', () => {
    render(
      <MemoryRouter>
        <ProfileCard octopus={makeOctopus()} screenX={400} screenY={300} onClose={vi.fn()} />
      </MemoryRouter>,
    );
    const link = screen.getByText('View Logs');
    expect(link).toBeInTheDocument();
    expect(link.closest('a')).toHaveAttribute('href', '/session/worker-alpha');
  });

  it('calls onClose when clicking backdrop', () => {
    const onClose = vi.fn();
    render(
      <MemoryRouter>
        <ProfileCard octopus={makeOctopus()} screenX={400} screenY={300} onClose={onClose} />
      </MemoryRouter>,
    );
    fireEvent.click(screen.getByTestId('profile-card-backdrop'));
    expect(onClose).toHaveBeenCalled();
  });

  it('does not call onClose when clicking the card itself', () => {
    const onClose = vi.fn();
    render(
      <MemoryRouter>
        <ProfileCard octopus={makeOctopus()} screenX={400} screenY={300} onClose={onClose} />
      </MemoryRouter>,
    );
    fireEvent.click(screen.getByTestId('profile-card'));
    expect(onClose).not.toHaveBeenCalled();
  });

  it('clamps card position to viewport', () => {
    render(
      <MemoryRouter>
        <ProfileCard octopus={makeOctopus()} screenX={2000} screenY={2000} onClose={vi.fn()} />
      </MemoryRouter>,
    );
    const card = screen.getByTestId('profile-card');
    const left = parseInt(card.style.left);
    const top = parseInt(card.style.top);
    expect(left).toBeLessThanOrEqual(window.innerWidth);
    expect(top).toBeLessThanOrEqual(window.innerHeight);
  });
});
