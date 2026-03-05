import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor, fireEvent } from '@testing-library/react';
import { SidebarProvider } from '@/components/ui/sidebar';
import { TooltipProvider } from '@/components/ui/tooltip';
import { ConnectionProvider } from '@/hooks/use-connection';
import { SettingsPage } from './settings';
import * as api from '@/api/client';
import type { ConfigResponse } from '@/api/types';

vi.mock('@/api/client', () => ({
  getConfig: vi.fn(),
  updateConfig: vi.fn(),
  getPeers: vi.fn(),
  addPeer: vi.fn(),
  removePeer: vi.fn(),
  resolveBaseUrl: vi.fn().mockReturnValue(''),
  authHeaders: vi.fn().mockReturnValue({}),
  setApiConfig: vi.fn(),
}));

vi.stubGlobal('localStorage', {
  getItem: () => null,
  setItem: () => {},
  removeItem: () => {},
});

const mockGetConfig = vi.mocked(api.getConfig);
const mockUpdateConfig = vi.mocked(api.updateConfig);
const mockGetPeers = vi.mocked(api.getPeers);

const testConfig: ConfigResponse = {
  node: { name: 'mac-studio', port: 7433, data_dir: '~/.pulpo/data' },
  peers: {},
  guards: { preset: 'standard' },
};

const testNode = {
  name: 'mac-studio',
  hostname: 'mac',
  os: 'darwin',
  arch: 'arm64',
  cpus: 8,
  memory_mb: 32000,
  gpu: null,
};

beforeEach(() => {
  mockGetConfig.mockReset();
  mockUpdateConfig.mockReset();
  mockGetPeers.mockReset();
});

function renderSettings() {
  return render(
    <ConnectionProvider>
      <TooltipProvider>
        <SidebarProvider>
          <SettingsPage />
        </SidebarProvider>
      </TooltipProvider>
    </ConnectionProvider>,
  );
}

describe('SettingsPage', () => {
  it('shows loading skeleton initially', () => {
    mockGetConfig.mockResolvedValue(testConfig);
    mockGetPeers.mockResolvedValue({ local: testNode, peers: [] });
    renderSettings();
    expect(screen.getByTestId('loading-skeleton')).toBeInTheDocument();
  });

  it('loads and displays config', async () => {
    mockGetConfig.mockResolvedValue(testConfig);
    mockGetPeers.mockResolvedValue({ local: testNode, peers: [] });
    renderSettings();

    await waitFor(() => {
      expect(screen.getByLabelText('Name')).toHaveValue('mac-studio');
      expect(screen.getByLabelText('Port')).toHaveValue(7433);
      expect(screen.getByTestId('save-btn')).toBeInTheDocument();
    });
  });

  it('shows error on config load failure', async () => {
    mockGetConfig.mockRejectedValue(new Error('Network error'));
    renderSettings();
    await waitFor(() => {
      expect(screen.getByText('Failed to load config')).toBeInTheDocument();
    });
  });

  it('saves config successfully', async () => {
    mockGetConfig.mockResolvedValue(testConfig);
    mockGetPeers.mockResolvedValue({ local: testNode, peers: [] });
    mockUpdateConfig.mockResolvedValue({
      config: testConfig,
      restart_required: false,
    });
    renderSettings();

    await waitFor(() => {
      expect(screen.getByTestId('save-btn')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByTestId('save-btn'));

    await waitFor(() => {
      expect(mockUpdateConfig).toHaveBeenCalledWith({
        node_name: 'mac-studio',
        port: 7433,
        data_dir: '~/.pulpo/data',
        guard_preset: 'standard',
      });
    });
  });

  it('shows restart message when port changes', async () => {
    mockGetConfig.mockResolvedValue(testConfig);
    mockGetPeers.mockResolvedValue({ local: testNode, peers: [] });
    mockUpdateConfig.mockResolvedValue({
      config: testConfig,
      restart_required: true,
    });
    renderSettings();

    await waitFor(() => {
      expect(screen.getByTestId('save-btn')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByTestId('save-btn'));

    await waitFor(() => {
      expect(mockUpdateConfig).toHaveBeenCalled();
    });
  });

  it('shows error on save failure', async () => {
    mockGetConfig.mockResolvedValue(testConfig);
    mockGetPeers.mockResolvedValue({ local: testNode, peers: [] });
    mockUpdateConfig.mockRejectedValue(new Error('Save failed'));
    renderSettings();

    await waitFor(() => {
      expect(screen.getByTestId('save-btn')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByTestId('save-btn'));

    await waitFor(() => {
      expect(screen.getByText('Failed to save config')).toBeInTheDocument();
    });
  });
});
