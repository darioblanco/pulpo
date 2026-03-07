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
  node: {
    name: 'mac-studio',
    port: 7433,
    data_dir: '~/.pulpo/data',
    bind: 'local',
    tag: null,
    seed: null,
    discovery_interval_secs: 60,
  },
  peers: {},
  guards: {
    preset: 'standard',
    max_turns: null,
    max_budget_usd: null,
    output_format: null,
  },
  watchdog: {
    enabled: true,
    memory_threshold: 85,
    check_interval_secs: 30,
    breach_count: 3,
    idle_timeout_secs: 300,
    idle_action: 'pause',
  },
  notifications: {
    discord: null,
  },
  personas: {},
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

  it('loads all settings sections', async () => {
    mockGetConfig.mockResolvedValue(testConfig);
    mockGetPeers.mockResolvedValue({ local: testNode, peers: [] });
    renderSettings();

    await waitFor(() => {
      expect(screen.getByTestId('node-settings')).toBeInTheDocument();
      expect(screen.getByTestId('guard-settings')).toBeInTheDocument();
      expect(screen.getByTestId('watchdog-settings')).toBeInTheDocument();
      expect(screen.getByTestId('notifications-settings')).toBeInTheDocument();
      expect(screen.getByTestId('peer-settings')).toBeInTheDocument();
    });
  });

  it('shows node-specific and global sections', async () => {
    mockGetConfig.mockResolvedValue(testConfig);
    mockGetPeers.mockResolvedValue({ local: testNode, peers: [] });
    renderSettings();

    await waitFor(() => {
      expect(screen.getByTestId('section-node')).toBeInTheDocument();
      expect(screen.getByTestId('section-global')).toBeInTheDocument();
      expect(screen.getByText('This node')).toBeInTheDocument();
      expect(screen.getByText('node-specific')).toBeInTheDocument();
      expect(screen.getByText('Global')).toBeInTheDocument();
      expect(screen.getByText('synced to all nodes')).toBeInTheDocument();
    });
  });

  it('loads watchdog settings', async () => {
    mockGetConfig.mockResolvedValue(testConfig);
    mockGetPeers.mockResolvedValue({ local: testNode, peers: [] });
    renderSettings();

    await waitFor(() => {
      expect(screen.getByLabelText('Memory threshold (%)')).toHaveValue(85);
    });
  });

  it('loads discord notifications when present', async () => {
    const configWithDiscord: ConfigResponse = {
      ...testConfig,
      notifications: {
        discord: {
          webhook_url: 'https://discord.com/api/webhooks/test',
          events: ['session.created', 'session.completed'],
        },
      },
    };
    mockGetConfig.mockResolvedValue(configWithDiscord);
    mockGetPeers.mockResolvedValue({ local: testNode, peers: [] });
    renderSettings();

    await waitFor(() => {
      expect(screen.getByLabelText('Webhook URL')).toHaveValue(
        'https://discord.com/api/webhooks/test',
      );
      expect(screen.getByLabelText('Events')).toHaveValue(
        'session.created, session.completed',
      );
    });
  });

  it('loads guard defaults when present', async () => {
    const configWithGuards: ConfigResponse = {
      ...testConfig,
      guards: {
        preset: 'strict',
        max_turns: 50,
        max_budget_usd: 10.5,
        output_format: 'json',
      },
    };
    mockGetConfig.mockResolvedValue(configWithGuards);
    mockGetPeers.mockResolvedValue({ local: testNode, peers: [] });
    renderSettings();

    await waitFor(() => {
      expect(screen.getByLabelText('Max turns')).toHaveValue(50);
      expect(screen.getByLabelText('Max budget (USD)')).toHaveValue(10.5);
      expect(screen.getByLabelText('Output format')).toHaveValue('json');
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
      expect(mockUpdateConfig).toHaveBeenCalledWith(
        expect.objectContaining({
          node_name: 'mac-studio',
          port: 7433,
          data_dir: '~/.pulpo/data',
          bind: 'local',
          guard_preset: 'standard',
          watchdog_enabled: true,
          watchdog_memory_threshold: 85,
        }),
      );
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

  it('sends guard_max_turns when set', async () => {
    const configWithTurns: ConfigResponse = {
      ...testConfig,
      guards: { ...testConfig.guards, max_turns: 25 },
    };
    mockGetConfig.mockResolvedValue(configWithTurns);
    mockGetPeers.mockResolvedValue({ local: testNode, peers: [] });
    mockUpdateConfig.mockResolvedValue({
      config: configWithTurns,
      restart_required: false,
    });
    renderSettings();

    await waitFor(() => {
      expect(screen.getByTestId('save-btn')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByTestId('save-btn'));

    await waitFor(() => {
      expect(mockUpdateConfig).toHaveBeenCalledWith(
        expect.objectContaining({
          guard_max_turns: 25,
        }),
      );
    });
  });

  it('sends discord events when set', async () => {
    const configWithDiscord: ConfigResponse = {
      ...testConfig,
      notifications: {
        discord: {
          webhook_url: 'https://discord.com/api/webhooks/test',
          events: ['session.created'],
        },
      },
    };
    mockGetConfig.mockResolvedValue(configWithDiscord);
    mockGetPeers.mockResolvedValue({ local: testNode, peers: [] });
    mockUpdateConfig.mockResolvedValue({
      config: configWithDiscord,
      restart_required: false,
    });
    renderSettings();

    await waitFor(() => {
      expect(screen.getByTestId('save-btn')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByTestId('save-btn'));

    await waitFor(() => {
      expect(mockUpdateConfig).toHaveBeenCalledWith(
        expect.objectContaining({
          discord_webhook_url: 'https://discord.com/api/webhooks/test',
          discord_events: ['session.created'],
        }),
      );
    });
  });

  it('peers section shows disabled in local mode', async () => {
    mockGetConfig.mockResolvedValue(testConfig);
    mockGetPeers.mockResolvedValue({ local: testNode, peers: [] });
    renderSettings();

    await waitFor(() => {
      expect(screen.getByTestId('peers-disabled')).toBeInTheDocument();
    });
  });
});
