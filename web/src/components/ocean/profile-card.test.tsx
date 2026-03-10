import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
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
    model: null,
    mode: 'autonomous',
    workdir: '/home/user/projects/pulpo/web',
    unrestricted: false,
    createdAt: '2026-01-01T00:00:00Z',
    lastOutputAt: null,
    interventionReason: null,
    prompt: 'Fix the auth bug',
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

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date('2026-01-01T00:12:00Z'));
});

afterEach(() => {
  vi.useRealTimers();
});

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

  it('renders Open Session button when onAttach provided', () => {
    render(
      <MemoryRouter>
        <ProfileCard
          octopus={makeOctopus()}
          screenX={400}
          screenY={300}
          onClose={vi.fn()}
          onAttach={vi.fn()}
        />
      </MemoryRouter>,
    );
    expect(screen.getByText('Open Session')).toBeInTheDocument();
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

  // --- New field tests ---

  it('renders shortened model when present', () => {
    render(
      <MemoryRouter>
        <ProfileCard
          octopus={makeOctopus({ model: 'claude-sonnet-4-20250514' })}
          screenX={400}
          screenY={300}
          onClose={vi.fn()}
        />
      </MemoryRouter>,
    );
    expect(screen.getByTestId('profile-model')).toHaveTextContent('sonnet-4');
  });

  it('does not render model when null', () => {
    render(
      <MemoryRouter>
        <ProfileCard
          octopus={makeOctopus({ model: null })}
          screenX={400}
          screenY={300}
          onClose={vi.fn()}
        />
      </MemoryRouter>,
    );
    expect(screen.queryByTestId('profile-model')).not.toBeInTheDocument();
  });

  it('renders mode', () => {
    render(
      <MemoryRouter>
        <ProfileCard
          octopus={makeOctopus({ mode: 'chat' })}
          screenX={400}
          screenY={300}
          onClose={vi.fn()}
        />
      </MemoryRouter>,
    );
    expect(screen.getByText('chat')).toBeInTheDocument();
  });

  it('renders truncated workdir', () => {
    render(
      <MemoryRouter>
        <ProfileCard octopus={makeOctopus()} screenX={400} screenY={300} onClose={vi.fn()} />
      </MemoryRouter>,
    );
    const el = screen.getByTestId('profile-workdir');
    expect(el).toHaveTextContent('…/pulpo/web');
  });

  it('renders short workdir without truncation', () => {
    render(
      <MemoryRouter>
        <ProfileCard
          octopus={makeOctopus({ workdir: '/tmp/repo' })}
          screenX={400}
          screenY={300}
          onClose={vi.fn()}
        />
      </MemoryRouter>,
    );
    const el = screen.getByTestId('profile-workdir');
    expect(el).toHaveTextContent('/tmp/repo');
  });

  it('renders duration', () => {
    render(
      <MemoryRouter>
        <ProfileCard octopus={makeOctopus()} screenX={400} screenY={300} onClose={vi.fn()} />
      </MemoryRouter>,
    );
    const el = screen.getByTestId('profile-duration');
    expect(el).toHaveTextContent('running for 12m');
  });

  it('shows completed duration for terminal statuses', () => {
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
    const el = screen.getByTestId('profile-duration');
    expect(el).toHaveTextContent('completed after 12m');
  });

  it('renders last active when present', () => {
    render(
      <MemoryRouter>
        <ProfileCard
          octopus={makeOctopus({ lastOutputAt: '2026-01-01T00:10:00Z' })}
          screenX={400}
          screenY={300}
          onClose={vi.fn()}
        />
      </MemoryRouter>,
    );
    const el = screen.getByTestId('profile-last-active');
    expect(el).toHaveTextContent('2m ago');
  });

  it('does not render last active when null', () => {
    render(
      <MemoryRouter>
        <ProfileCard octopus={makeOctopus()} screenX={400} screenY={300} onClose={vi.fn()} />
      </MemoryRouter>,
    );
    expect(screen.queryByTestId('profile-last-active')).not.toBeInTheDocument();
  });

  it('shows unrestricted badge when true', () => {
    render(
      <MemoryRouter>
        <ProfileCard
          octopus={makeOctopus({ unrestricted: true })}
          screenX={400}
          screenY={300}
          onClose={vi.fn()}
        />
      </MemoryRouter>,
    );
    expect(screen.getByTestId('unrestricted-badge')).toBeInTheDocument();
  });

  it('hides unrestricted badge when false', () => {
    render(
      <MemoryRouter>
        <ProfileCard octopus={makeOctopus()} screenX={400} screenY={300} onClose={vi.fn()} />
      </MemoryRouter>,
    );
    expect(screen.queryByTestId('unrestricted-badge')).not.toBeInTheDocument();
  });

  it('shows intervention reason when present', () => {
    render(
      <MemoryRouter>
        <ProfileCard
          octopus={makeOctopus({ interventionReason: 'Memory limit exceeded' })}
          screenX={400}
          screenY={300}
          onClose={vi.fn()}
        />
      </MemoryRouter>,
    );
    const el = screen.getByTestId('profile-intervention');
    expect(el).toHaveTextContent('Memory limit exceeded');
  });

  it('hides intervention reason when null', () => {
    render(
      <MemoryRouter>
        <ProfileCard octopus={makeOctopus()} screenX={400} screenY={300} onClose={vi.fn()} />
      </MemoryRouter>,
    );
    expect(screen.queryByTestId('profile-intervention')).not.toBeInTheDocument();
  });

  it('renders Attach button when onAttach is provided', () => {
    render(
      <MemoryRouter>
        <ProfileCard
          octopus={makeOctopus()}
          screenX={400}
          screenY={300}
          onClose={vi.fn()}
          onAttach={vi.fn()}
        />
      </MemoryRouter>,
    );
    expect(screen.getByTestId('attach-button')).toBeInTheDocument();
  });

  it('does not render Attach button when onAttach is not provided', () => {
    render(
      <MemoryRouter>
        <ProfileCard octopus={makeOctopus()} screenX={400} screenY={300} onClose={vi.fn()} />
      </MemoryRouter>,
    );
    expect(screen.queryByTestId('attach-button')).not.toBeInTheDocument();
  });

  it('calls onAttach with session name when Attach is clicked', () => {
    const onAttach = vi.fn();
    render(
      <MemoryRouter>
        <ProfileCard
          octopus={makeOctopus()}
          screenX={400}
          screenY={300}
          onClose={vi.fn()}
          onAttach={onAttach}
        />
      </MemoryRouter>,
    );
    fireEvent.click(screen.getByTestId('attach-button'));
    expect(onAttach).toHaveBeenCalledWith('worker-alpha');
  });

  it('renders last active as just now for recent output', () => {
    render(
      <MemoryRouter>
        <ProfileCard
          octopus={makeOctopus({ lastOutputAt: '2026-01-01T00:11:55Z' })}
          screenX={400}
          screenY={300}
          onClose={vi.fn()}
        />
      </MemoryRouter>,
    );
    expect(screen.getByTestId('profile-last-active')).toHaveTextContent('just now');
  });

  it('renders last active in hours for old output', () => {
    render(
      <MemoryRouter>
        <ProfileCard
          octopus={makeOctopus({ lastOutputAt: '2025-12-31T22:00:00Z' })}
          screenX={400}
          screenY={300}
          onClose={vi.fn()}
        />
      </MemoryRouter>,
    );
    expect(screen.getByTestId('profile-last-active')).toHaveTextContent('2h ago');
  });

  it('renders non-claude model without shortening', () => {
    render(
      <MemoryRouter>
        <ProfileCard
          octopus={makeOctopus({ model: 'gpt-4o-mini' })}
          screenX={400}
          screenY={300}
          onClose={vi.fn()}
        />
      </MemoryRouter>,
    );
    expect(screen.getByTestId('profile-model')).toHaveTextContent('gpt-4o-mini');
  });

  // --- Kill / Delete action tests ---

  it('shows Kill button for running sessions when onKill provided', () => {
    render(
      <MemoryRouter>
        <ProfileCard
          octopus={makeOctopus({ status: 'running' })}
          screenX={400}
          screenY={300}
          onClose={vi.fn()}
          onKill={vi.fn()}
        />
      </MemoryRouter>,
    );
    expect(screen.getByTestId('kill-button')).toBeInTheDocument();
  });

  it('shows Kill button for creating sessions', () => {
    render(
      <MemoryRouter>
        <ProfileCard
          octopus={makeOctopus({ status: 'creating' })}
          screenX={400}
          screenY={300}
          onClose={vi.fn()}
          onKill={vi.fn()}
        />
      </MemoryRouter>,
    );
    expect(screen.getByTestId('kill-button')).toBeInTheDocument();
  });

  it('does not show Kill button for completed sessions', () => {
    render(
      <MemoryRouter>
        <ProfileCard
          octopus={makeOctopus({ status: 'completed' })}
          screenX={400}
          screenY={300}
          onClose={vi.fn()}
          onKill={vi.fn()}
        />
      </MemoryRouter>,
    );
    expect(screen.queryByTestId('kill-button')).not.toBeInTheDocument();
  });

  it('does not show Kill button when onKill not provided', () => {
    render(
      <MemoryRouter>
        <ProfileCard
          octopus={makeOctopus({ status: 'running' })}
          screenX={400}
          screenY={300}
          onClose={vi.fn()}
        />
      </MemoryRouter>,
    );
    expect(screen.queryByTestId('kill-button')).not.toBeInTheDocument();
  });

  it('calls onKill with session name when Kill clicked', () => {
    const onKill = vi.fn();
    render(
      <MemoryRouter>
        <ProfileCard
          octopus={makeOctopus({ status: 'running' })}
          screenX={400}
          screenY={300}
          onClose={vi.fn()}
          onKill={onKill}
        />
      </MemoryRouter>,
    );
    fireEvent.click(screen.getByTestId('kill-button'));
    expect(onKill).toHaveBeenCalledWith('worker-alpha');
  });

  it('shows Delete button for stale sessions when onDelete provided', () => {
    render(
      <MemoryRouter>
        <ProfileCard
          octopus={makeOctopus({ status: 'stale' })}
          screenX={400}
          screenY={300}
          onClose={vi.fn()}
          onDelete={vi.fn()}
        />
      </MemoryRouter>,
    );
    expect(screen.getByTestId('delete-button')).toBeInTheDocument();
  });

  it('shows Delete button for dead sessions', () => {
    render(
      <MemoryRouter>
        <ProfileCard
          octopus={makeOctopus({ status: 'dead' })}
          screenX={400}
          screenY={300}
          onClose={vi.fn()}
          onDelete={vi.fn()}
        />
      </MemoryRouter>,
    );
    expect(screen.getByTestId('delete-button')).toBeInTheDocument();
  });

  it('shows Delete button for completed sessions', () => {
    render(
      <MemoryRouter>
        <ProfileCard
          octopus={makeOctopus({ status: 'completed' })}
          screenX={400}
          screenY={300}
          onClose={vi.fn()}
          onDelete={vi.fn()}
        />
      </MemoryRouter>,
    );
    expect(screen.getByTestId('delete-button')).toBeInTheDocument();
  });

  it('does not show Delete button for running sessions', () => {
    render(
      <MemoryRouter>
        <ProfileCard
          octopus={makeOctopus({ status: 'running' })}
          screenX={400}
          screenY={300}
          onClose={vi.fn()}
          onDelete={vi.fn()}
        />
      </MemoryRouter>,
    );
    expect(screen.queryByTestId('delete-button')).not.toBeInTheDocument();
  });

  it('does not show Delete button when onDelete not provided', () => {
    render(
      <MemoryRouter>
        <ProfileCard
          octopus={makeOctopus({ status: 'stale' })}
          screenX={400}
          screenY={300}
          onClose={vi.fn()}
        />
      </MemoryRouter>,
    );
    expect(screen.queryByTestId('delete-button')).not.toBeInTheDocument();
  });

  it('calls onDelete with session name when Delete clicked', () => {
    const onDelete = vi.fn();
    render(
      <MemoryRouter>
        <ProfileCard
          octopus={makeOctopus({ status: 'dead' })}
          screenX={400}
          screenY={300}
          onClose={vi.fn()}
          onDelete={onDelete}
        />
      </MemoryRouter>,
    );
    fireEvent.click(screen.getByTestId('delete-button'));
    expect(onDelete).toHaveBeenCalledWith('worker-alpha');
  });
});
