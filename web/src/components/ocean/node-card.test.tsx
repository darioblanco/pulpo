import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { NodeCard } from './node-card';
import type { NodeLandmark } from './engine/world';

function makeNode(overrides: Partial<NodeLandmark> = {}): NodeLandmark {
  return {
    name: 'mac-studio',
    isLocal: true,
    status: 'online',
    x: 0,
    y: 230,
    color: '#f472b6',
    sessionCount: 3,
    ...overrides,
  };
}

describe('NodeCard', () => {
  it('renders node name', () => {
    render(<NodeCard node={makeNode()} screenX={400} screenY={300} onClose={vi.fn()} />);
    expect(screen.getByText('mac-studio')).toBeInTheDocument();
  });

  it('renders online status', () => {
    render(<NodeCard node={makeNode()} screenX={400} screenY={300} onClose={vi.fn()} />);
    expect(screen.getByText('Online')).toBeInTheDocument();
  });

  it('renders offline status', () => {
    render(
      <NodeCard
        node={makeNode({ status: 'offline' })}
        screenX={400}
        screenY={300}
        onClose={vi.fn()}
      />,
    );
    expect(screen.getByText('Offline')).toBeInTheDocument();
  });

  it('renders unknown status', () => {
    render(
      <NodeCard
        node={makeNode({ status: 'unknown' })}
        screenX={400}
        screenY={300}
        onClose={vi.fn()}
      />,
    );
    expect(screen.getByText('Unknown')).toBeInTheDocument();
  });

  it('renders session count', () => {
    render(
      <NodeCard
        node={makeNode({ sessionCount: 5 })}
        screenX={400}
        screenY={300}
        onClose={vi.fn()}
      />,
    );
    expect(screen.getByTestId('node-session-count').textContent).toBe('5');
  });

  it('shows local indicator for local node', () => {
    render(
      <NodeCard node={makeNode({ isLocal: true })} screenX={400} screenY={300} onClose={vi.fn()} />,
    );
    expect(screen.getByText('(local)')).toBeInTheDocument();
  });

  it('does not show local indicator for peer node', () => {
    render(
      <NodeCard
        node={makeNode({ isLocal: false })}
        screenX={400}
        screenY={300}
        onClose={vi.fn()}
      />,
    );
    expect(screen.queryByText('(local)')).not.toBeInTheDocument();
  });

  it('shows peer node type', () => {
    render(
      <NodeCard
        node={makeNode({ isLocal: false })}
        screenX={400}
        screenY={300}
        onClose={vi.fn()}
      />,
    );
    expect(screen.getByText('Peer node')).toBeInTheDocument();
  });

  it('shows local node type', () => {
    render(
      <NodeCard node={makeNode({ isLocal: true })} screenX={400} screenY={300} onClose={vi.fn()} />,
    );
    expect(screen.getByText('Local node')).toBeInTheDocument();
  });

  it('calls onClose when clicking backdrop', () => {
    const onClose = vi.fn();
    render(<NodeCard node={makeNode()} screenX={400} screenY={300} onClose={onClose} />);
    fireEvent.click(screen.getByTestId('node-card-backdrop'));
    expect(onClose).toHaveBeenCalled();
  });

  it('does not call onClose when clicking the card itself', () => {
    const onClose = vi.fn();
    render(<NodeCard node={makeNode()} screenX={400} screenY={300} onClose={onClose} />);
    fireEvent.click(screen.getByTestId('node-card'));
    expect(onClose).not.toHaveBeenCalled();
  });

  it('clamps card position to viewport', () => {
    render(<NodeCard node={makeNode()} screenX={2000} screenY={2000} onClose={vi.fn()} />);
    const card = screen.getByTestId('node-card');
    const left = parseInt(card.style.left);
    const top = parseInt(card.style.top);
    expect(left).toBeLessThanOrEqual(window.innerWidth);
    expect(top).toBeLessThanOrEqual(window.innerHeight);
  });

  it('renders status dot', () => {
    render(<NodeCard node={makeNode()} screenX={400} screenY={300} onClose={vi.fn()} />);
    expect(screen.getByTestId('node-status-dot')).toBeInTheDocument();
  });
});
