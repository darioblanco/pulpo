import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { cleanup, render, screen, fireEvent, waitFor } from '@testing-library/svelte';
import Page from './+page.svelte';

const mockGoto = vi.fn();
vi.mock('$app/navigation', () => ({
  goto: (...args: unknown[]) => mockGoto(...args),
}));

vi.mock('$app/state', () => ({
  page: {
    url: new URL('http://localhost/connect'),
  },
}));

const mockTestConnection = vi.fn();
vi.mock('$lib/connection', () => ({
  testConnection: (...args: unknown[]) => mockTestConnection(...args),
}));

const mockGetPeers = vi.fn();
vi.mock('$lib/api', () => ({
  getPeers: (...args: unknown[]) => mockGetPeers(...args),
}));

const mockSetBaseUrl = vi.fn();
const mockSetAuthToken = vi.fn();
const mockAddSavedConnection = vi.fn();
const mockRemoveSavedConnection = vi.fn();
let mockSavedConnections: { name: string; url: string; token?: string; lastConnected: string }[] =
  [];

vi.mock('$lib/stores/connection.svelte', () => ({
  setBaseUrl: (...args: unknown[]) => mockSetBaseUrl(...args),
  setAuthToken: (...args: unknown[]) => mockSetAuthToken(...args),
  addSavedConnection: (...args: unknown[]) => mockAddSavedConnection(...args),
  removeSavedConnection: (...args: unknown[]) => mockRemoveSavedConnection(...args),
  getSavedConnections: () => mockSavedConnections,
  loadSavedConnections: vi.fn(),
}));

beforeEach(() => {
  mockGoto.mockReset();
  mockTestConnection.mockReset();
  mockSetBaseUrl.mockReset();
  mockSetAuthToken.mockReset();
  mockAddSavedConnection.mockReset();
  mockRemoveSavedConnection.mockReset();
  mockGetPeers.mockReset();
  mockSavedConnections = [];
  // Default: no peers
  mockGetPeers.mockResolvedValue({ local: { name: 'local' }, peers: [] });
});

afterEach(() => {
  cleanup();
});

describe('connect page', () => {
  it('renders connection form with token field', () => {
    render(Page);

    expect(screen.getByText('Connect to Pulpo')).toBeTruthy();
    expect(screen.getByText('Connect')).toBeTruthy();
    expect(screen.getByPlaceholderText('Leave empty for local connections')).toBeTruthy();
  });

  it('connects successfully and navigates to dashboard', async () => {
    mockTestConnection.mockResolvedValue({ name: 'mac-mini' });
    render(Page);

    const input = screen.getByPlaceholderText('http://mac-mini:7433');
    await fireEvent.input(input, { target: { value: 'http://mac-mini:7433' } });

    const connectBtn = screen.getByText('Connect');
    await fireEvent.click(connectBtn);

    await waitFor(() => {
      expect(mockTestConnection).toHaveBeenCalledWith('http://mac-mini:7433', undefined);
      expect(mockSetBaseUrl).toHaveBeenCalledWith('http://mac-mini:7433');
      expect(mockAddSavedConnection).toHaveBeenCalledWith(
        expect.objectContaining({ name: 'mac-mini', url: 'http://mac-mini:7433' }),
      );
      expect(mockGoto).toHaveBeenCalledWith('/');
    });
  });

  it('connects with token and stores it', async () => {
    mockTestConnection.mockResolvedValue({ name: 'mac-mini' });
    render(Page);

    const urlInput = screen.getByPlaceholderText('http://mac-mini:7433');
    await fireEvent.input(urlInput, { target: { value: 'http://mac-mini:7433' } });

    const tokenInput = screen.getByPlaceholderText('Leave empty for local connections');
    await fireEvent.input(tokenInput, { target: { value: 'my-secret' } });

    const connectBtn = screen.getByText('Connect');
    await fireEvent.click(connectBtn);

    await waitFor(() => {
      expect(mockTestConnection).toHaveBeenCalledWith('http://mac-mini:7433', 'my-secret');
      expect(mockSetAuthToken).toHaveBeenCalledWith('my-secret');
      expect(mockAddSavedConnection).toHaveBeenCalledWith(
        expect.objectContaining({ token: 'my-secret' }),
      );
    });
  });

  it('shows error on connection failure', async () => {
    mockTestConnection.mockRejectedValue(new Error('Connection refused'));
    render(Page);

    const input = screen.getByPlaceholderText('http://mac-mini:7433');
    await fireEvent.input(input, { target: { value: 'http://bad:7433' } });

    const connectBtn = screen.getByText('Connect');
    await fireEvent.click(connectBtn);

    await waitFor(() => {
      expect(screen.getByText('Failed to connect to http://bad:7433')).toBeTruthy();
    });
  });

  it('does not submit when URL is empty', async () => {
    render(Page);

    const connectBtn = screen.getByText('Connect');
    await fireEvent.click(connectBtn);

    expect(mockTestConnection).not.toHaveBeenCalled();
  });

  it('renders saved connections', () => {
    mockSavedConnections = [
      { name: 'Mac Mini', url: 'http://mac-mini:7433', lastConnected: '2026-01-01' },
    ];
    render(Page);

    expect(screen.getByText('Saved Connections')).toBeTruthy();
    expect(screen.getByText('Mac Mini')).toBeTruthy();
    expect(screen.getByText('http://mac-mini:7433')).toBeTruthy();
  });

  it('connects via saved connection click', async () => {
    mockTestConnection.mockResolvedValue({ name: 'mac-mini' });
    mockSavedConnections = [
      { name: 'Mac Mini', url: 'http://mac-mini:7433', lastConnected: '2026-01-01' },
    ];
    render(Page);

    const savedItem = screen.getByText('Mac Mini');
    await fireEvent.click(savedItem);

    await waitFor(() => {
      expect(mockTestConnection).toHaveBeenCalledWith('http://mac-mini:7433', undefined);
    });
  });

  it('connects via saved connection with token', async () => {
    mockTestConnection.mockResolvedValue({ name: 'mac-mini' });
    mockSavedConnections = [
      {
        name: 'Mac Mini',
        url: 'http://mac-mini:7433',
        token: 'saved-token',
        lastConnected: '2026-01-01',
      },
    ];
    render(Page);

    const savedItem = screen.getByText('Mac Mini');
    await fireEvent.click(savedItem);

    await waitFor(() => {
      expect(mockTestConnection).toHaveBeenCalledWith('http://mac-mini:7433', 'saved-token');
      expect(mockSetAuthToken).toHaveBeenCalledWith('saved-token');
    });
  });

  it('removes saved connection', async () => {
    mockSavedConnections = [
      { name: 'Mac Mini', url: 'http://mac-mini:7433', lastConnected: '2026-01-01' },
    ];
    render(Page);

    const removeBtn = screen.getByText('Remove');
    await fireEvent.click(removeBtn);

    expect(mockRemoveSavedConnection).toHaveBeenCalledWith('http://mac-mini:7433');
  });

  it('shows discovered peers', async () => {
    mockGetPeers.mockResolvedValue({
      local: { name: 'local' },
      peers: [
        {
          name: 'win-pc',
          address: '192.168.1.100:7433',
          status: 'online',
          node_info: null,
          session_count: null,
          source: 'discovered',
        },
      ],
    });
    render(Page);

    await waitFor(() => {
      expect(screen.getByText('Nearby Devices')).toBeTruthy();
      expect(screen.getByText('win-pc')).toBeTruthy();
      expect(screen.getByText('192.168.1.100:7433')).toBeTruthy();
      expect(screen.getByText('discovered')).toBeTruthy();
    });
  });

  it('filters out configured peers from nearby devices', async () => {
    mockGetPeers.mockResolvedValue({
      local: { name: 'local' },
      peers: [
        {
          name: 'configured-node',
          address: '10.0.0.1:7433',
          status: 'online',
          node_info: null,
          session_count: null,
          source: 'configured',
        },
      ],
    });
    render(Page);

    // Wait for the getPeers call to resolve
    await waitFor(() => {
      expect(mockGetPeers).toHaveBeenCalled();
    });

    expect(screen.queryByText('Nearby Devices')).toBeNull();
  });

  it('filters out offline discovered peers', async () => {
    mockGetPeers.mockResolvedValue({
      local: { name: 'local' },
      peers: [
        {
          name: 'offline-node',
          address: '10.0.0.1:7433',
          status: 'offline',
          node_info: null,
          session_count: null,
          source: 'discovered',
        },
      ],
    });
    render(Page);

    await waitFor(() => {
      expect(mockGetPeers).toHaveBeenCalled();
    });

    expect(screen.queryByText('Nearby Devices')).toBeNull();
  });

  it('connects to discovered peer on click', async () => {
    mockTestConnection.mockResolvedValue({ name: 'win-pc' });
    mockGetPeers.mockResolvedValue({
      local: { name: 'local' },
      peers: [
        {
          name: 'win-pc',
          address: '192.168.1.100:7433',
          status: 'online',
          node_info: null,
          session_count: null,
          source: 'discovered',
        },
      ],
    });
    render(Page);

    await waitFor(() => {
      expect(screen.getByText('win-pc')).toBeTruthy();
    });

    await fireEvent.click(screen.getByText('win-pc'));

    await waitFor(() => {
      expect(mockTestConnection).toHaveBeenCalledWith('http://192.168.1.100:7433', undefined);
    });
  });

  it('extracts token from URL query parameter', async () => {
    // Re-mock $app/state with a token in the URL
    const stateModule = await import('$app/state');
    (stateModule.page as unknown as { url: URL }).url = new URL(
      'http://localhost/connect?token=url-token',
    );

    render(Page);

    const tokenInput = screen.getByPlaceholderText(
      'Leave empty for local connections',
    ) as HTMLInputElement;
    expect(tokenInput.value).toBe('url-token');

    // Restore original
    (stateModule.page as unknown as { url: URL }).url = new URL('http://localhost/connect');
  });

  it('handles getPeers failure gracefully', async () => {
    mockGetPeers.mockRejectedValue(new Error('Network error'));
    render(Page);

    await waitFor(() => {
      expect(mockGetPeers).toHaveBeenCalled();
    });

    // Should not crash, no nearby devices shown
    expect(screen.queryByText('Nearby Devices')).toBeNull();
  });
});
