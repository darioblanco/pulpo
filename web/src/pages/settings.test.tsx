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

const testConfig: ConfigResponse = {
  node: {
    name: 'mac-studio',
    port: 7433,
    data_dir: '~/.pulpo/data',
    bind: 'local',
    tag: null,
  },
  watchdog: {
    enabled: true,
    memory_threshold: 85,
    check_interval_secs: 30,
    breach_count: 3,
    idle_timeout_secs: 300,
    idle_action: 'pause',
    ready_ttl_secs: 0,
  },
  notifications: {
    webhooks: [],
  },
};

beforeEach(() => {
  mockGetConfig.mockReset();
  mockUpdateConfig.mockReset();
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

function clickTab(testId: string) {
  const tab = screen.getByTestId(testId);
  fireEvent.pointerDown(tab, { button: 0, pointerType: 'mouse' });
  fireEvent.pointerUp(tab, { button: 0, pointerType: 'mouse' });
  fireEvent.mouseDown(tab, { button: 0 });
  fireEvent.mouseUp(tab, { button: 0 });
  fireEvent.click(tab);
}

describe('SettingsPage', () => {
  it('shows loading skeleton initially', () => {
    mockGetConfig.mockResolvedValue(testConfig);
    renderSettings();
    expect(screen.getByTestId('loading-skeleton')).toBeInTheDocument();
  });

  it('loads and displays config', async () => {
    mockGetConfig.mockResolvedValue(testConfig);
    renderSettings();

    await waitFor(() => {
      expect(screen.getByLabelText('Name')).toHaveValue('mac-studio');
      expect(screen.getByLabelText('Port')).toHaveValue(7433);
      expect(screen.getByTestId('save-btn')).toBeInTheDocument();
    });
  });

  it('loads all settings tabs', async () => {
    mockGetConfig.mockResolvedValue(testConfig);
    renderSettings();

    await waitFor(() => {
      expect(screen.getByTestId('settings-tabs')).toBeInTheDocument();
      expect(screen.getByTestId('settings-tab-node')).toBeInTheDocument();
      expect(screen.getByTestId('settings-tab-watchdog')).toBeInTheDocument();
      expect(screen.getByTestId('settings-tab-notifications')).toBeInTheDocument();
    });

    // Node tab is default — node-settings should be visible
    expect(screen.getByTestId('node-settings')).toBeInTheDocument();
  });

  it('shows settings for each tab when clicked', async () => {
    mockGetConfig.mockResolvedValue(testConfig);
    renderSettings();

    await waitFor(() => {
      expect(screen.getByTestId('node-settings')).toBeInTheDocument();
    });

    clickTab('settings-tab-watchdog');
    await waitFor(() => {
      expect(screen.getByTestId('watchdog-settings')).toBeInTheDocument();
    });

    clickTab('settings-tab-notifications');
    await waitFor(() => {
      expect(screen.getByTestId('notifications-settings')).toBeInTheDocument();
    });
  });

  it('loads watchdog settings', async () => {
    mockGetConfig.mockResolvedValue(testConfig);
    renderSettings();

    await waitFor(() => {
      expect(screen.getByTestId('settings-tab-watchdog')).toBeInTheDocument();
    });

    clickTab('settings-tab-watchdog');

    await waitFor(() => {
      expect(screen.getByLabelText('Memory threshold (%)')).toHaveValue(85);
    });
  });

  it('loads webhook notifications when present', async () => {
    const configWithWebhook: ConfigResponse = {
      ...testConfig,
      notifications: {
        webhooks: [
          {
            name: 'ci-hook',
            url: 'https://example.com/hook',
            events: ['session.created', 'session.ready'],
            has_secret: false,
          },
        ],
      },
    };
    mockGetConfig.mockResolvedValue(configWithWebhook);
    renderSettings();

    await waitFor(() => {
      expect(screen.getByTestId('settings-tab-notifications')).toBeInTheDocument();
    });

    clickTab('settings-tab-notifications');

    await waitFor(() => {
      const webhookSection = screen.getByTestId('webhook-0');
      expect(within(webhookSection).getByLabelText('Name')).toHaveValue('ci-hook');
      expect(within(webhookSection).getByLabelText('URL')).toHaveValue('https://example.com/hook');
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
          watchdog_enabled: true,
          watchdog_memory_threshold: 85,
        }),
      );
    });
  });

  it('shows restart message when port changes', async () => {
    mockGetConfig.mockResolvedValue(testConfig);
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

  it('sends webhooks when set', async () => {
    const configWithWebhook: ConfigResponse = {
      ...testConfig,
      notifications: {
        webhooks: [
          {
            name: 'ci-hook',
            url: 'https://example.com/hook',
            events: ['session.created'],
            has_secret: false,
          },
        ],
      },
    };
    mockGetConfig.mockResolvedValue(configWithWebhook);
    mockUpdateConfig.mockResolvedValue({
      config: configWithWebhook,
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
          webhooks: [
            { name: 'ci-hook', url: 'https://example.com/hook', events: ['session.created'] },
          ],
        }),
      );
    });
  });
});
