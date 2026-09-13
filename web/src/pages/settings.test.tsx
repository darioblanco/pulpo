import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import { SidebarProvider } from '@/components/ui/sidebar';
import { TooltipProvider } from '@/components/ui/tooltip';
import { ConnectionProvider } from '@/hooks/use-connection';
import { SettingsPage } from './settings';
import * as api from '@/api/client';
import type { ConfigResponse } from '@/api/types';

vi.mock('@/api/client', () => ({
  getConfig: vi.fn(),
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

const testConfig: ConfigResponse = {
  node: {
    name: 'mac-studio',
    port: 7433,
    data_dir: '~/.pulpo/data',
    bind: 'local',
  },
  watchdog: {
    enabled: true,
    check_interval_secs: 30,
    idle_timeout_secs: 300,
    idle_action: 'pause',
    idle_threshold_secs: 60,
    extra_waiting_patterns: [],
  },
  notifications: {
    webhooks: [],
  },
};

beforeEach(() => {
  mockGetConfig.mockReset();
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
    renderSettings();
    expect(screen.getByTestId('loading-skeleton')).toBeInTheDocument();
  });

  it('loads and displays the effective config read-only', async () => {
    mockGetConfig.mockResolvedValue(testConfig);
    renderSettings();

    await waitFor(() => {
      expect(screen.getByTestId('section-node')).toBeInTheDocument();
    });

    expect(screen.getByText('mac-studio')).toBeInTheDocument();
    expect(screen.getByText('7433')).toBeInTheDocument();
    expect(screen.getByText('~/.pulpo/data')).toBeInTheDocument();
    expect(screen.getByText('local')).toBeInTheDocument();

    // No editable inputs or save button — this is a read-only view.
    expect(screen.queryByRole('textbox')).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /save/i })).not.toBeInTheDocument();
  });

  it('shows the watchdog section', async () => {
    mockGetConfig.mockResolvedValue(testConfig);
    renderSettings();

    await waitFor(() => {
      expect(screen.getByTestId('section-watchdog')).toBeInTheDocument();
    });

    expect(screen.getByText('pause')).toBeInTheDocument();
    expect(screen.getByText('300')).toBeInTheDocument();
  });

  it('shows extra waiting patterns when present', async () => {
    mockGetConfig.mockResolvedValue({
      ...testConfig,
      watchdog: { ...testConfig.watchdog, extra_waiting_patterns: ['custom>'] },
    });
    renderSettings();

    await waitFor(() => {
      expect(screen.getByText('custom>')).toBeInTheDocument();
    });
  });

  it('shows a message when no webhooks are configured', async () => {
    mockGetConfig.mockResolvedValue(testConfig);
    renderSettings();

    await waitFor(() => {
      expect(screen.getByTestId('no-webhooks')).toBeInTheDocument();
    });
  });

  it('lists configured webhooks when present', async () => {
    mockGetConfig.mockResolvedValue({
      ...testConfig,
      notifications: {
        webhooks: [
          {
            name: 'ci-hook',
            url: 'https://example.com/hook',
            events: ['session.created', 'session.ready'],
          },
        ],
      },
    });
    renderSettings();

    await waitFor(() => {
      const section = screen.getByTestId('webhook-ci-hook');
      expect(section).toBeInTheDocument();
      expect(section).toHaveTextContent('https://example.com/hook');
      expect(section).toHaveTextContent('session.created, session.ready');
    });
  });

  it('shows "all" for a webhook with no event filter', async () => {
    mockGetConfig.mockResolvedValue({
      ...testConfig,
      notifications: {
        webhooks: [{ name: 'logs-hook', url: 'https://logs.example.com', events: [] }],
      },
    });
    renderSettings();

    await waitFor(() => {
      const section = screen.getByTestId('webhook-logs-hook');
      expect(section).toHaveTextContent('all');
    });
  });

  it('shows min severity when set', async () => {
    mockGetConfig.mockResolvedValue({
      ...testConfig,
      notifications: {
        webhooks: [
          {
            name: 'crit-hook',
            url: 'https://example.com/crit',
            events: [],
            min_severity: 'critical',
          },
        ],
      },
    });
    renderSettings();

    await waitFor(() => {
      expect(screen.getByTestId('webhook-crit-hook')).toHaveTextContent('critical');
    });
  });

  it('shows the edit-the-file hint', async () => {
    mockGetConfig.mockResolvedValue(testConfig);
    renderSettings();

    await waitFor(() => {
      expect(screen.getByTestId('config-hint')).toHaveTextContent('config.toml');
    });
  });

  it('shows error on config load failure', async () => {
    mockGetConfig.mockRejectedValue(new Error('Network error'));
    renderSettings();
    await waitFor(() => {
      expect(screen.getByText('Failed to load config')).toBeInTheDocument();
    });
  });
});
