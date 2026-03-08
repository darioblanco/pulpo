import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/react';
import { Octopus } from './octopus';

describe('Octopus', () => {
  it('renders an octopus SVG', () => {
    render(
      <svg>
        <Octopus x={50} y={50} status="running" name="api-fix" />
      </svg>,
    );
    expect(screen.getByTestId('octopus-api-fix')).toBeInTheDocument();
  });

  it('shows session name as tooltip text', () => {
    render(
      <svg>
        <Octopus x={50} y={50} status="running" name="my-session" />
      </svg>,
    );
    const octopus = screen.getByTestId('octopus-my-session');
    expect(octopus.querySelector('title')?.textContent).toBe('my-session');
  });

  it('applies running animation class', () => {
    render(
      <svg>
        <Octopus x={50} y={50} status="running" name="test" />
      </svg>,
    );
    const octopus = screen.getByTestId('octopus-test');
    expect(octopus.classList.contains('octopus-running')).toBe(true);
  });

  it('applies stale animation class', () => {
    render(
      <svg>
        <Octopus x={50} y={50} status="stale" name="test" />
      </svg>,
    );
    const octopus = screen.getByTestId('octopus-test');
    expect(octopus.classList.contains('octopus-stale')).toBe(true);
  });

  it('applies completed animation class', () => {
    render(
      <svg>
        <Octopus x={50} y={50} status="completed" name="test" />
      </svg>,
    );
    const octopus = screen.getByTestId('octopus-test');
    expect(octopus.classList.contains('octopus-completed')).toBe(true);
  });

  it('applies dead animation class', () => {
    render(
      <svg>
        <Octopus x={50} y={50} status="dead" name="test" />
      </svg>,
    );
    const octopus = screen.getByTestId('octopus-test');
    expect(octopus.classList.contains('octopus-dead')).toBe(true);
  });

  it('applies creating animation class', () => {
    render(
      <svg>
        <Octopus x={50} y={50} status="creating" name="test" />
      </svg>,
    );
    const octopus = screen.getByTestId('octopus-test');
    expect(octopus.classList.contains('octopus-creating')).toBe(true);
  });

  it('shows ink badge when ink is provided', () => {
    render(
      <svg>
        <Octopus x={50} y={50} status="running" name="test" ink="reviewer" />
      </svg>,
    );
    expect(screen.getByText('reviewer')).toBeInTheDocument();
  });

  it('shows provider badge', () => {
    render(
      <svg>
        <Octopus x={50} y={50} status="running" name="test" provider="claude" />
      </svg>,
    );
    expect(screen.getByText('claude')).toBeInTheDocument();
  });

  it('uses transform for positioning', () => {
    render(
      <svg>
        <Octopus x={100} y={200} status="running" name="pos-test" />
      </svg>,
    );
    const octopus = screen.getByTestId('octopus-pos-test');
    expect(octopus.getAttribute('transform')).toContain('translate(100');
    expect(octopus.getAttribute('transform')).toContain('200');
  });

  it('shows waiting indicator when waiting_for_input is true', () => {
    render(
      <svg>
        <Octopus x={50} y={50} status="running" name="test" waitingForInput />
      </svg>,
    );
    expect(screen.getByTestId('waiting-indicator')).toBeInTheDocument();
  });

  it('does not show waiting indicator by default', () => {
    render(
      <svg>
        <Octopus x={50} y={50} status="running" name="test" />
      </svg>,
    );
    expect(screen.queryByTestId('waiting-indicator')).not.toBeInTheDocument();
  });
});
