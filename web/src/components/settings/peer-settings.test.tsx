import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { PeerSettings } from './peer-settings';
import * as api from '@/api/client';
import type { PeerInfo } from '@/api/types';

vi.mock('@/api/client', () => ({
  addPeer: vi.fn(),
  removePeer: vi.fn(),
  resolveBaseUrl: vi.fn().mockReturnValue(''),
  authHeaders: vi.fn().mockReturnValue({}),
  setApiConfig: vi.fn(),
}));

const mockAddPeer = vi.mocked(api.addPeer);
const mockRemovePeer = vi.mocked(api.removePeer);

beforeEach(() => {
  mockAddPeer.mockReset();
  mockRemovePeer.mockReset();
});

const peers: PeerInfo[] = [
  {
    name: 'node-a',
    address: '10.0.0.1:7433',
    status: 'online',
    node_info: null,
    session_count: null,
  },
  {
    name: 'node-b',
    address: '10.0.0.2:7433',
    status: 'offline',
    node_info: null,
    session_count: null,
  },
];

describe('PeerSettings', () => {
  it('renders peers', () => {
    render(<PeerSettings peers={peers} onUpdate={vi.fn()} />);
    expect(screen.getByTestId('peer-settings')).toBeInTheDocument();
    expect(screen.getByTestId('peer-node-a')).toBeInTheDocument();
    expect(screen.getByTestId('peer-node-b')).toBeInTheDocument();
  });

  it('shows empty message when no peers', () => {
    render(<PeerSettings peers={[]} onUpdate={vi.fn()} />);
    expect(screen.getByText('No peers configured.')).toBeInTheDocument();
  });

  it('adds a peer', async () => {
    const newPeers = [
      ...peers,
      {
        name: 'node-c',
        address: '10.0.0.3:7433',
        status: 'unknown' as const,
        node_info: null,
        session_count: null,
      },
    ];
    mockAddPeer.mockResolvedValue({ local: null as never, peers: newPeers });
    const onUpdate = vi.fn();
    render(<PeerSettings peers={peers} onUpdate={onUpdate} />);

    fireEvent.change(screen.getByLabelText('Peer name'), { target: { value: 'node-c' } });
    fireEvent.change(screen.getByLabelText('Peer address'), { target: { value: '10.0.0.3:7433' } });
    fireEvent.click(screen.getByTestId('add-peer-btn'));

    await waitFor(() => {
      expect(mockAddPeer).toHaveBeenCalledWith('node-c', '10.0.0.3:7433');
      expect(onUpdate).toHaveBeenCalledWith(newPeers);
    });
  });

  it('does not add peer with empty fields', () => {
    render(<PeerSettings peers={peers} onUpdate={vi.fn()} />);
    fireEvent.click(screen.getByTestId('add-peer-btn'));
    expect(mockAddPeer).not.toHaveBeenCalled();
  });

  it('shows error on add peer failure', async () => {
    mockAddPeer.mockRejectedValue(new Error('Connection refused'));
    render(<PeerSettings peers={peers} onUpdate={vi.fn()} />);

    fireEvent.change(screen.getByLabelText('Peer name'), { target: { value: 'bad' } });
    fireEvent.change(screen.getByLabelText('Peer address'), { target: { value: 'bad:7433' } });
    fireEvent.click(screen.getByTestId('add-peer-btn'));

    await waitFor(() => {
      expect(screen.getByText('Connection refused')).toBeInTheDocument();
    });
  });

  it('shows generic error for non-Error failure', async () => {
    mockAddPeer.mockRejectedValue('string error');
    render(<PeerSettings peers={peers} onUpdate={vi.fn()} />);

    fireEvent.change(screen.getByLabelText('Peer name'), { target: { value: 'bad' } });
    fireEvent.change(screen.getByLabelText('Peer address'), { target: { value: 'bad:7433' } });
    fireEvent.click(screen.getByTestId('add-peer-btn'));

    await waitFor(() => {
      expect(screen.getByText('Failed to add peer')).toBeInTheDocument();
    });
  });

  it('removes a peer', async () => {
    mockRemovePeer.mockResolvedValue(undefined);
    const onUpdate = vi.fn();
    render(<PeerSettings peers={peers} onUpdate={onUpdate} />);
    fireEvent.click(screen.getByTestId('remove-peer-node-a'));

    await waitFor(() => {
      expect(mockRemovePeer).toHaveBeenCalledWith('node-a');
      expect(onUpdate).toHaveBeenCalledWith([peers[1]]);
    });
  });
});
