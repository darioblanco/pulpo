import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent, within } from '@testing-library/react';
import { NotificationsSettings } from './notifications-settings';
import type { WebhookFormData } from './notifications-settings';

const mockEnable = vi.fn();
const mockDisable = vi.fn();

vi.mock('@/hooks/use-push-notifications', () => ({
  usePushNotifications: vi.fn(() => ({
    isSupported: true,
    isEnabled: false,
    isLoading: false,
    permission: 'default' as NotificationPermission,
    enable: mockEnable,
    disable: mockDisable,
  })),
}));

import { usePushNotifications } from '@/hooks/use-push-notifications';

const mockUsePush = vi.mocked(usePushNotifications);

const defaults = {
  discordWebhookUrl: '',
  onDiscordWebhookUrlChange: vi.fn(),
  discordEvents: '',
  onDiscordEventsChange: vi.fn(),
  webhooks: [] as WebhookFormData[],
  onWebhooksChange: vi.fn(),
};

describe('NotificationsSettings', () => {
  it('renders tabs with webhooks first', () => {
    render(<NotificationsSettings {...defaults} />);
    expect(screen.getByTestId('notifications-settings')).toBeInTheDocument();
    expect(screen.getByTestId('tab-webhooks')).toBeInTheDocument();
    expect(screen.getByTestId('tab-discord')).toBeInTheDocument();
  });

  it('shows inactive badge when no webhooks configured', () => {
    render(<NotificationsSettings {...defaults} />);
    const badges = screen.getAllByText('inactive');
    expect(badges.length).toBe(2);
  });

  it('shows active badge for webhooks when configured', () => {
    render(
      <NotificationsSettings
        {...defaults}
        webhooks={[
          { name: 'hook', url: 'https://a.com', events: [], has_secret: false, secret: '' },
        ]}
      />,
    );
    const activeBadges = screen.getAllByText('active');
    expect(activeBadges.length).toBe(1);
  });

  it('shows active badge for discord when webhook URL set', () => {
    render(
      <NotificationsSettings
        {...defaults}
        discordWebhookUrl="https://discord.com/api/webhooks/test"
      />,
    );
    const activeBadges = screen.getAllByText('active');
    expect(activeBadges.length).toBe(1);
  });

  it('shows both active when both configured', () => {
    render(
      <NotificationsSettings
        {...defaults}
        discordWebhookUrl="https://discord.com/api/webhooks/test"
        webhooks={[
          { name: 'hook', url: 'https://a.com', events: [], has_secret: false, secret: '' },
        ]}
      />,
    );
    const activeBadges = screen.getAllByText('active');
    expect(activeBadges.length).toBe(2);
  });

  it('renders discord fields in discord tab', () => {
    render(<NotificationsSettings {...defaults} />);
    fireEvent.click(screen.getByTestId('tab-discord'));
    const discordContent = screen.getByTestId('discord-content');
    expect(within(discordContent).getByLabelText('Webhook URL')).toHaveValue('');
  });

  it('displays existing discord values', () => {
    render(
      <NotificationsSettings
        {...defaults}
        discordWebhookUrl="https://discord.com/api/webhooks/test"
        discordEvents="session.created, session.ready"
      />,
    );
    fireEvent.click(screen.getByTestId('tab-discord'));
    const discordContent = screen.getByTestId('discord-content');
    expect(within(discordContent).getByLabelText('Webhook URL')).toHaveValue(
      'https://discord.com/api/webhooks/test',
    );
    expect(within(discordContent).getByLabelText('Events')).toHaveValue(
      'session.created, session.ready',
    );
  });

  it('calls onDiscordWebhookUrlChange', () => {
    const onDiscordWebhookUrlChange = vi.fn();
    render(
      <NotificationsSettings {...defaults} onDiscordWebhookUrlChange={onDiscordWebhookUrlChange} />,
    );
    fireEvent.click(screen.getByTestId('tab-discord'));
    const discordContent = screen.getByTestId('discord-content');
    fireEvent.change(within(discordContent).getByLabelText('Webhook URL'), {
      target: { value: 'https://discord.com/api/webhooks/new' },
    });
    expect(onDiscordWebhookUrlChange).toHaveBeenCalledWith('https://discord.com/api/webhooks/new');
  });

  it('calls onDiscordEventsChange', () => {
    const onDiscordEventsChange = vi.fn();
    render(<NotificationsSettings {...defaults} onDiscordEventsChange={onDiscordEventsChange} />);
    fireEvent.click(screen.getByTestId('tab-discord'));
    const discordContent = screen.getByTestId('discord-content');
    fireEvent.change(within(discordContent).getByLabelText('Events'), {
      target: { value: 'session.lost' },
    });
    expect(onDiscordEventsChange).toHaveBeenCalledWith('session.lost');
  });

  it('adds a webhook', () => {
    const onWebhooksChange = vi.fn();
    render(<NotificationsSettings {...defaults} onWebhooksChange={onWebhooksChange} />);
    fireEvent.click(screen.getByTestId('add-webhook-btn'));
    expect(onWebhooksChange).toHaveBeenCalledWith([
      { name: '', url: '', events: [], has_secret: false, secret: '' },
    ]);
  });

  it('removes a webhook', () => {
    const onWebhooksChange = vi.fn();
    render(
      <NotificationsSettings
        {...defaults}
        webhooks={[
          { name: 'hook-1', url: 'https://a.com', events: [], has_secret: false, secret: '' },
          { name: 'hook-2', url: 'https://b.com', events: [], has_secret: false, secret: '' },
        ]}
        onWebhooksChange={onWebhooksChange}
      />,
    );
    fireEvent.click(screen.getByTestId('remove-webhook-0'));
    expect(onWebhooksChange).toHaveBeenCalledWith([
      { name: 'hook-2', url: 'https://b.com', events: [], has_secret: false, secret: '' },
    ]);
  });

  it('updates webhook name', () => {
    const onWebhooksChange = vi.fn();
    render(
      <NotificationsSettings
        {...defaults}
        webhooks={[{ name: '', url: '', events: [], has_secret: false, secret: '' }]}
        onWebhooksChange={onWebhooksChange}
      />,
    );
    const webhookSection = screen.getByTestId('webhook-0');
    fireEvent.change(within(webhookSection).getByLabelText('Name'), {
      target: { value: 'my-hook' },
    });
    expect(onWebhooksChange).toHaveBeenCalledWith([
      { name: 'my-hook', url: '', events: [], has_secret: false, secret: '' },
    ]);
  });

  it('updates webhook url', () => {
    const onWebhooksChange = vi.fn();
    render(
      <NotificationsSettings
        {...defaults}
        webhooks={[{ name: 'hook', url: '', events: [], has_secret: false, secret: '' }]}
        onWebhooksChange={onWebhooksChange}
      />,
    );
    const webhookSection = screen.getByTestId('webhook-0');
    fireEvent.change(within(webhookSection).getByLabelText('URL'), {
      target: { value: 'https://example.com' },
    });
    expect(onWebhooksChange).toHaveBeenCalledWith([
      { name: 'hook', url: 'https://example.com', events: [], has_secret: false, secret: '' },
    ]);
  });

  it('updates webhook events', () => {
    const onWebhooksChange = vi.fn();
    render(
      <NotificationsSettings
        {...defaults}
        webhooks={[
          { name: 'hook', url: 'https://a.com', events: [], has_secret: false, secret: '' },
        ]}
        onWebhooksChange={onWebhooksChange}
      />,
    );
    const webhookSection = screen.getByTestId('webhook-0');
    fireEvent.change(within(webhookSection).getByLabelText('Events'), {
      target: { value: 'stopped, ready' },
    });
    expect(onWebhooksChange).toHaveBeenCalledWith([
      {
        name: 'hook',
        url: 'https://a.com',
        events: ['stopped', 'ready'],
        has_secret: false,
        secret: '',
      },
    ]);
  });

  it('updates webhook secret', () => {
    const onWebhooksChange = vi.fn();
    render(
      <NotificationsSettings
        {...defaults}
        webhooks={[
          { name: 'hook', url: 'https://a.com', events: [], has_secret: false, secret: '' },
        ]}
        onWebhooksChange={onWebhooksChange}
      />,
    );
    const webhookSection = screen.getByTestId('webhook-0');
    fireEvent.change(within(webhookSection).getByLabelText('Secret'), {
      target: { value: 'my-secret' },
    });
    expect(onWebhooksChange).toHaveBeenCalledWith([
      {
        name: 'hook',
        url: 'https://a.com',
        events: [],
        has_secret: false,
        secret: 'my-secret',
      },
    ]);
  });

  it('shows existing secret hint', () => {
    render(
      <NotificationsSettings
        {...defaults}
        webhooks={[
          { name: 'hook', url: 'https://a.com', events: [], has_secret: true, secret: '' },
        ]}
      />,
    );
    expect(screen.getByText(/A secret is configured/)).toBeInTheDocument();
  });

  it('renders webhook details', () => {
    render(
      <NotificationsSettings
        {...defaults}
        webhooks={[
          {
            name: 'ci-hook',
            url: 'https://ci.example.com',
            events: ['stopped'],
            has_secret: true,
            secret: '',
          },
        ]}
      />,
    );
    expect(screen.getByTestId('webhook-0')).toBeInTheDocument();
    expect(screen.getByDisplayValue('ci-hook')).toBeInTheDocument();
    expect(screen.getByDisplayValue('https://ci.example.com')).toBeInTheDocument();
  });

  it('renders push notifications section', () => {
    render(<NotificationsSettings {...defaults} />);
    expect(screen.getByTestId('push-section')).toBeInTheDocument();
    expect(screen.getByTestId('push-toggle')).toBeInTheDocument();
    expect(screen.getByText('Disabled')).toBeInTheDocument();
    expect(screen.getByText(/Receive notifications when sessions finish/)).toBeInTheDocument();
  });

  it('shows Enabled label when push is enabled', () => {
    mockUsePush.mockReturnValue({
      isSupported: true,
      isEnabled: true,
      isLoading: false,
      permission: 'granted',
      enable: mockEnable,
      disable: mockDisable,
    });
    render(<NotificationsSettings {...defaults} />);
    expect(screen.getByText('Enabled')).toBeInTheDocument();
  });

  it('shows Blocked label when permission denied', () => {
    mockUsePush.mockReturnValue({
      isSupported: true,
      isEnabled: false,
      isLoading: false,
      permission: 'denied',
      enable: mockEnable,
      disable: mockDisable,
    });
    render(<NotificationsSettings {...defaults} />);
    expect(screen.getByText('Blocked')).toBeInTheDocument();
  });

  it('shows Not supported label when push not supported', () => {
    mockUsePush.mockReturnValue({
      isSupported: false,
      isEnabled: false,
      isLoading: false,
      permission: 'default',
      enable: mockEnable,
      disable: mockDisable,
    });
    render(<NotificationsSettings {...defaults} />);
    expect(screen.getByText('Not supported')).toBeInTheDocument();
  });

  it('disables toggle when not supported', () => {
    mockUsePush.mockReturnValue({
      isSupported: false,
      isEnabled: false,
      isLoading: false,
      permission: 'default',
      enable: mockEnable,
      disable: mockDisable,
    });
    render(<NotificationsSettings {...defaults} />);
    expect(screen.getByTestId('push-toggle')).toBeDisabled();
  });

  it('disables toggle when loading', () => {
    mockUsePush.mockReturnValue({
      isSupported: true,
      isEnabled: false,
      isLoading: true,
      permission: 'default',
      enable: mockEnable,
      disable: mockDisable,
    });
    render(<NotificationsSettings {...defaults} />);
    expect(screen.getByTestId('push-toggle')).toBeDisabled();
  });

  it('disables toggle when permission denied', () => {
    mockUsePush.mockReturnValue({
      isSupported: true,
      isEnabled: false,
      isLoading: false,
      permission: 'denied',
      enable: mockEnable,
      disable: mockDisable,
    });
    render(<NotificationsSettings {...defaults} />);
    expect(screen.getByTestId('push-toggle')).toBeDisabled();
  });

  it('calls enable when toggled on', () => {
    mockUsePush.mockReturnValue({
      isSupported: true,
      isEnabled: false,
      isLoading: false,
      permission: 'default',
      enable: mockEnable,
      disable: mockDisable,
    });
    render(<NotificationsSettings {...defaults} />);
    fireEvent.click(screen.getByTestId('push-toggle'));
    expect(mockEnable).toHaveBeenCalled();
  });

  it('calls disable when toggled off', () => {
    mockUsePush.mockReturnValue({
      isSupported: true,
      isEnabled: true,
      isLoading: false,
      permission: 'granted',
      enable: mockEnable,
      disable: mockDisable,
    });
    render(<NotificationsSettings {...defaults} />);
    fireEvent.click(screen.getByTestId('push-toggle'));
    expect(mockDisable).toHaveBeenCalled();
  });
});
