import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { SavedConnections } from './saved-connections';
import type { SavedConnection } from '@/hooks/use-connection';

const connections: SavedConnection[] = [
  { name: 'mac-studio', url: 'http://10.0.0.1:7433', lastConnected: '2026-01-01T00:00:00Z' },
  {
    name: 'linux-box',
    url: 'http://10.0.0.2:7433',
    token: 'tok',
    lastConnected: '2026-01-02T00:00:00Z',
  },
];

describe('SavedConnections', () => {
  it('renders nothing when empty', () => {
    const { container } = render(
      <SavedConnections connections={[]} onSelect={vi.fn()} onRemove={vi.fn()} />,
    );
    expect(container.innerHTML).toBe('');
  });

  it('renders saved connections', () => {
    render(<SavedConnections connections={connections} onSelect={vi.fn()} onRemove={vi.fn()} />);
    expect(screen.getByTestId('saved-connections')).toBeInTheDocument();
    expect(screen.getByTestId('saved-mac-studio')).toBeInTheDocument();
    expect(screen.getByTestId('saved-linux-box')).toBeInTheDocument();
  });

  it('calls onSelect when clicked', () => {
    const onSelect = vi.fn();
    render(<SavedConnections connections={connections} onSelect={onSelect} onRemove={vi.fn()} />);
    fireEvent.click(screen.getByTestId('select-mac-studio'));
    expect(onSelect).toHaveBeenCalledWith(connections[0]);
  });

  it('calls onRemove when remove clicked', () => {
    const onRemove = vi.fn();
    render(<SavedConnections connections={connections} onSelect={vi.fn()} onRemove={onRemove} />);
    fireEvent.click(screen.getByTestId('remove-linux-box'));
    expect(onRemove).toHaveBeenCalledWith('http://10.0.0.2:7433');
  });
});
