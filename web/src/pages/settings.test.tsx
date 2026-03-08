import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor, fireEvent, within } from '@testing-library/react';
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
    unrestricted: false,
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
    webhooks: [],
  },
  inks: {},
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
      expect(screen.getByTestId('ink-settings')).toBeInTheDocument();
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
        webhooks: [],
      },
    };
    mockGetConfig.mockResolvedValue(configWithDiscord);
    mockGetPeers.mockResolvedValue({ local: testNode, peers: [] });
    renderSettings();

    await waitFor(() => {
      expect(screen.getByTestId('tab-discord')).toBeInTheDocument();
    });

    fireEvent.click(screen.getByTestId('tab-discord'));

    await waitFor(() => {
      const discordContent = screen.getByTestId('discord-content');
      expect(within(discordContent).getByLabelText('Webhook URL')).toHaveValue(
        'https://discord.com/api/webhooks/test',
      );
      expect(within(discordContent).getByLabelText('Events')).toHaveValue(
        'session.created, session.completed',
      );
    });
  });

  it('loads guard defaults when present', async () => {
    const configWithGuards: ConfigResponse = {
      ...testConfig,
      guards: {
        unrestricted: true,
      },
    };
    mockGetConfig.mockResolvedValue(configWithGuards);
    mockGetPeers.mockResolvedValue({ local: testNode, peers: [] });
    renderSettings();

    await waitFor(() => {
      const toggle = screen.getByTestId('guard-unrestricted-toggle');
      expect(toggle).toHaveAttribute('data-state', 'checked');
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
          unrestricted: false,
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

  it('sends discord events when set', async () => {
    const configWithDiscord: ConfigResponse = {
      ...testConfig,
      notifications: {
        discord: {
          webhook_url: 'https://discord.com/api/webhooks/test',
          events: ['session.created'],
        },
        webhooks: [],
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

  it('loads and displays inks from config', async () => {
    const configWithInks: ConfigResponse = {
      ...testConfig,
      inks: {
        reviewer: {
          description: 'Code reviewer',
          provider: 'claude',
          model: null,
          mode: 'interactive',
          unrestricted: false,
          instructions: null,
        },
      },
    };
    mockGetConfig.mockResolvedValue(configWithInks);
    mockGetPeers.mockResolvedValue({ local: testNode, peers: [] });
    renderSettings();

    await waitFor(() => {
      expect(screen.getByTestId('ink-settings')).toBeInTheDocument();
      expect(screen.getByTestId('ink-reviewer')).toBeInTheDocument();
    });
  });

  it('saves inks in config update', async () => {
    const configWithInks: ConfigResponse = {
      ...testConfig,
      inks: {
        coder: {
          description: 'Coder ink',
          provider: 'claude',
          model: null,
          mode: 'autonomous',
          unrestricted: false,
          instructions: null,
        },
      },
    };
    mockGetConfig.mockResolvedValue(configWithInks);
    mockGetPeers.mockResolvedValue({ local: testNode, peers: [] });
    mockUpdateConfig.mockResolvedValue({
      config: configWithInks,
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
          inks: {
            coder: expect.objectContaining({
              description: 'Coder ink',
              provider: 'claude',
            }),
          },
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
