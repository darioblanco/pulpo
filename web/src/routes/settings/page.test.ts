import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { cleanup, render, screen, fireEvent } from '@testing-library/svelte';
import Page from './+page.svelte';
import * as api from '$lib/api';
import type { ConfigResponse, PeersResponse } from '$lib/api';

vi.mock('$lib/api', () => ({
  getConfig: vi.fn(),
  updateConfig: vi.fn(),
  getPeers: vi.fn(),
  addPeer: vi.fn(),
  removePeer: vi.fn(),
  getPairingUrl: vi.fn(),
}));

vi.mock('qrcode', () => ({
  default: {
    toString: vi.fn().mockResolvedValue('<svg>QR</svg>'),
  },
}));

const mockGetConfig = vi.mocked(api.getConfig);
const mockUpdateConfig = vi.mocked(api.updateConfig);
const mockGetPeers = vi.mocked(api.getPeers);
const mockAddPeer = vi.mocked(api.addPeer);
const mockRemovePeer = vi.mocked(api.removePeer);

function makeConfig(overrides: Partial<ConfigResponse> = {}): ConfigResponse {
  return {
    node: { name: 'mac-mini', port: 7433, data_dir: '~/.pulpo' },
    peers: {},
    guards: { preset: 'standard' },
    ...overrides,
  };
}

function makePeersResponse(peers: PeersResponse['peers'] = []): PeersResponse {
  return {
    local: {
      name: 'mac-mini',
      hostname: 'mac-mini.local',
      os: 'darwin',
      arch: 'aarch64',
      cpus: 8,
      memory_mb: 16384,
      gpu: null,
    },
    peers,
  };
}

afterEach(() => {
  cleanup();
});

beforeEach(() => {
  mockGetConfig.mockReset();
  mockUpdateConfig.mockReset();
  mockGetPeers.mockReset();
  mockAddPeer.mockReset();
  mockRemovePeer.mockReset();
  mockGetPeers.mockResolvedValue(makePeersResponse());
});

describe('settings page', () => {
  it('shows loading state initially', () => {
    mockGetConfig.mockReturnValue(new Promise(() => {}));

    render(Page);

    expect(screen.getByText('Loading config...')).toBeTruthy();
  });

  it('shows error on load failure', async () => {
    mockGetConfig.mockRejectedValue(new Error('Network error'));

    render(Page);

    await vi.waitFor(() => {
      expect(screen.getByText('Failed to load config')).toBeTruthy();
    });

    expect(screen.getByText('Retry')).toBeTruthy();
  });

  it('loads and displays config', async () => {
    mockGetConfig.mockResolvedValue(makeConfig());

    render(Page);

    await vi.waitFor(() => {
      const nameInput = screen.getByDisplayValue('mac-mini');
      expect(nameInput).toBeTruthy();
    });

    expect(screen.getByDisplayValue('7433')).toBeTruthy();
    expect(screen.getByDisplayValue('~/.pulpo')).toBeTruthy();
    expect(screen.getByText('Save')).toBeTruthy();
  });

  it('shows guard preset segmented control', async () => {
    mockGetConfig.mockResolvedValue(makeConfig());

    render(Page);

    await vi.waitFor(() => {
      expect(screen.getByText('strict')).toBeTruthy();
    });

    expect(screen.getByText('standard')).toBeTruthy();
    expect(screen.getByText('unrestricted')).toBeTruthy();
  });

  it('saves config on button click', async () => {
    mockGetConfig.mockResolvedValue(makeConfig());
    mockUpdateConfig.mockResolvedValue({
      config: makeConfig(),
      restart_required: false,
    });

    render(Page);

    await vi.waitFor(() => {
      expect(screen.getByText('Save')).toBeTruthy();
    });

    await fireEvent.click(screen.getByText('Save'));

    await vi.waitFor(() => {
      expect(mockUpdateConfig).toHaveBeenCalledWith({
        node_name: 'mac-mini',
        port: 7433,
        data_dir: '~/.pulpo',
        guard_preset: 'standard',
      });
    });

    expect(screen.getByText('Settings saved.')).toBeTruthy();
  });

  it('shows restart message when port changes', async () => {
    mockGetConfig.mockResolvedValue(makeConfig());
    mockUpdateConfig.mockResolvedValue({
      config: makeConfig({ node: { name: 'mac-mini', port: 9000, data_dir: '~/.pulpo' } }),
      restart_required: true,
    });

    render(Page);

    await vi.waitFor(() => {
      expect(screen.getByText('Save')).toBeTruthy();
    });

    await fireEvent.click(screen.getByText('Save'));

    await vi.waitFor(() => {
      expect(
        screen.getByText('Saved. Restart pulpod for port change to take effect.'),
      ).toBeTruthy();
    });
  });

  it('shows error on save failure', async () => {
    mockGetConfig.mockResolvedValue(makeConfig());
    mockUpdateConfig.mockRejectedValue(new Error('Save failed'));

    render(Page);

    await vi.waitFor(() => {
      expect(screen.getByText('Save')).toBeTruthy();
    });

    await fireEvent.click(screen.getByText('Save'));

    await vi.waitFor(() => {
      expect(screen.getByText('Failed to save config')).toBeTruthy();
    });
  });

  it('retries loading on retry button click', async () => {
    mockGetConfig.mockRejectedValueOnce(new Error('fail'));

    render(Page);

    await vi.waitFor(() => {
      expect(screen.getByText('Retry')).toBeTruthy();
    });

    mockGetConfig.mockResolvedValue(makeConfig());
    await fireEvent.click(screen.getByText('Retry'));

    await vi.waitFor(() => {
      expect(screen.getByDisplayValue('mac-mini')).toBeTruthy();
    });
  });

  it('edits form fields via input events', async () => {
    mockGetConfig.mockResolvedValue(makeConfig());
    mockUpdateConfig.mockResolvedValue({
      config: makeConfig({
        node: { name: 'new-node', port: 9000, data_dir: '/custom/data' },
      }),
      restart_required: false,
    });

    render(Page);

    await vi.waitFor(() => {
      expect(screen.getByDisplayValue('mac-mini')).toBeTruthy();
    });

    const nameInput = screen.getByDisplayValue('mac-mini') as HTMLInputElement;
    const portInput = screen.getByDisplayValue('7433') as HTMLInputElement;
    const dataDirInput = screen.getByDisplayValue('~/.pulpo') as HTMLInputElement;

    await fireEvent.input(nameInput, { target: { value: 'new-node' } });
    await fireEvent.input(portInput, { target: { value: '9000' } });
    await fireEvent.input(dataDirInput, { target: { value: '/custom/data' } });

    await fireEvent.click(screen.getByText('Save'));

    await vi.waitFor(() => {
      expect(mockUpdateConfig).toHaveBeenCalledWith({
        node_name: 'new-node',
        port: 9000,
        data_dir: '/custom/data',
        guard_preset: 'standard',
      });
    });
  });

  it('auto-closes toast after timeout', async () => {
    vi.useFakeTimers();
    mockGetConfig.mockResolvedValue(makeConfig());
    mockUpdateConfig.mockResolvedValue({
      config: makeConfig(),
      restart_required: false,
    });

    render(Page);

    await vi.waitFor(() => {
      expect(screen.getByText('Save')).toBeTruthy();
    });

    await fireEvent.click(screen.getByText('Save'));

    await vi.waitFor(() => {
      expect(screen.getByText('Settings saved.')).toBeTruthy();
    });

    // Advance past the 3s toast timeout
    await vi.advanceTimersByTimeAsync(3000);
    vi.useRealTimers();
  });

  it('changes guard preset via segmented control', async () => {
    mockGetConfig.mockResolvedValue(makeConfig());
    mockUpdateConfig.mockResolvedValue({
      config: makeConfig({
        guards: { preset: 'strict' },
      }),
      restart_required: false,
    });

    render(Page);

    await vi.waitFor(() => {
      expect(screen.getByText('strict')).toBeTruthy();
    });

    await fireEvent.click(screen.getByText('strict'));
    await fireEvent.click(screen.getByText('Save'));

    await vi.waitFor(() => {
      expect(mockUpdateConfig).toHaveBeenCalledWith(
        expect.objectContaining({ guard_preset: 'strict' }),
      );
    });
  });

  it('displays peers list', async () => {
    mockGetConfig.mockResolvedValue(makeConfig());
    mockGetPeers.mockResolvedValue(
      makePeersResponse([
        {
          name: 'remote-a',
          address: '10.0.0.1:7433',
          status: 'online',
          node_info: null,
          session_count: null,
        },
        {
          name: 'remote-b',
          address: '10.0.0.2:7433',
          status: 'offline',
          node_info: null,
          session_count: null,
        },
      ]),
    );

    render(Page);

    await vi.waitFor(() => {
      expect(screen.getByText('remote-a')).toBeTruthy();
    });

    expect(screen.getByText('10.0.0.1:7433')).toBeTruthy();
    expect(screen.getByText('remote-b')).toBeTruthy();
    expect(screen.getByText('10.0.0.2:7433')).toBeTruthy();
    expect(screen.getAllByText('Remove')).toHaveLength(2);
  });

  it('adds a peer via form', async () => {
    mockGetConfig.mockResolvedValue(makeConfig());
    mockAddPeer.mockResolvedValue(
      makePeersResponse([
        {
          name: 'new-node',
          address: '10.0.0.5:7433',
          status: 'unknown',
          node_info: null,
          session_count: null,
        },
      ]),
    );

    render(Page);

    await vi.waitFor(() => {
      expect(screen.getByText('Add')).toBeTruthy();
    });

    const nameInput = screen.getByPlaceholderText('remote-node');
    const addressInput = screen.getByPlaceholderText('10.0.0.1:7433');

    await fireEvent.input(nameInput, { target: { value: 'new-node' } });
    await fireEvent.input(addressInput, { target: { value: '10.0.0.5:7433' } });
    await fireEvent.click(screen.getByText('Add'));

    await vi.waitFor(() => {
      expect(mockAddPeer).toHaveBeenCalledWith('new-node', '10.0.0.5:7433');
    });

    expect(screen.getByText('Peer added.')).toBeTruthy();
  });

  it('does not add peer with empty fields', async () => {
    mockGetConfig.mockResolvedValue(makeConfig());

    render(Page);

    await vi.waitFor(() => {
      expect(screen.getByText('Add')).toBeTruthy();
    });

    await fireEvent.click(screen.getByText('Add'));

    expect(mockAddPeer).not.toHaveBeenCalled();
  });

  it('removes a peer', async () => {
    mockGetConfig.mockResolvedValue(makeConfig());
    mockGetPeers.mockResolvedValue(
      makePeersResponse([
        {
          name: 'old-node',
          address: '10.0.0.1:7433',
          status: 'offline',
          node_info: null,
          session_count: null,
        },
      ]),
    );
    mockRemovePeer.mockResolvedValue(undefined);

    render(Page);

    await vi.waitFor(() => {
      expect(screen.getByText('old-node')).toBeTruthy();
    });

    await fireEvent.click(screen.getByText('Remove'));

    await vi.waitFor(() => {
      expect(mockRemovePeer).toHaveBeenCalledWith('old-node');
    });

    expect(screen.getByText('Peer removed.')).toBeTruthy();
  });

  it('shows error toast on add peer failure', async () => {
    mockGetConfig.mockResolvedValue(makeConfig());
    mockAddPeer.mockRejectedValue(new Error('already exists'));

    render(Page);

    await vi.waitFor(() => {
      expect(screen.getByText('Add')).toBeTruthy();
    });

    const nameInput = screen.getByPlaceholderText('remote-node');
    const addressInput = screen.getByPlaceholderText('10.0.0.1:7433');

    await fireEvent.input(nameInput, { target: { value: 'dup' } });
    await fireEvent.input(addressInput, { target: { value: 'x:7433' } });
    await fireEvent.click(screen.getByText('Add'));

    await vi.waitFor(() => {
      expect(screen.getByText('already exists')).toBeTruthy();
    });
  });

  it('shows error toast on remove peer failure', async () => {
    mockGetConfig.mockResolvedValue(makeConfig());
    mockGetPeers.mockResolvedValue(
      makePeersResponse([
        {
          name: 'node',
          address: '10.0.0.1:7433',
          status: 'unknown',
          node_info: null,
          session_count: null,
        },
      ]),
    );
    mockRemovePeer.mockRejectedValue(new Error('not found'));

    render(Page);

    await vi.waitFor(() => {
      expect(screen.getByText('node')).toBeTruthy();
    });

    await fireEvent.click(screen.getByText('Remove'));

    await vi.waitFor(() => {
      expect(screen.getByText('not found')).toBeTruthy();
    });
  });

  it('shows Pair Device button', async () => {
    mockGetConfig.mockResolvedValue(makeConfig());

    render(Page);

    await vi.waitFor(() => {
      expect(screen.getByText('Device Pairing')).toBeTruthy();
    });

    expect(screen.getByText('Pair Device')).toBeTruthy();
  });

  it('shows QR code when Pair Device is clicked', async () => {
    const mockGetPairingUrl = vi.mocked(api.getPairingUrl);
    mockGetPairingUrl.mockResolvedValue({ url: 'http://mac-mini:7433/?token=abc' });
    mockGetConfig.mockResolvedValue(makeConfig());

    render(Page);

    await vi.waitFor(() => {
      expect(screen.getByText('Pair Device')).toBeTruthy();
    });

    await fireEvent.click(screen.getByText('Pair Device'));

    await vi.waitFor(() => {
      expect(screen.getByText('Hide QR Code')).toBeTruthy();
    });
  });
});
