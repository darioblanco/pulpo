import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { NotificationsSettings } from './notifications-settings';

const defaults = {
  discordWebhookUrl: '',
  onDiscordWebhookUrlChange: vi.fn(),
  discordEvents: '',
  onDiscordEventsChange: vi.fn(),
};

describe('NotificationsSettings', () => {
  it('renders discord section', () => {
    render(<NotificationsSettings {...defaults} />);
    expect(screen.getByTestId('notifications-settings')).toBeInTheDocument();
    expect(screen.getByText('Discord')).toBeInTheDocument();
    expect(screen.getByLabelText('Webhook URL')).toHaveValue('');
    expect(screen.getByLabelText('Events')).toHaveValue('');
  });

  it('displays existing values', () => {
    render(
      <NotificationsSettings
        {...defaults}
        discordWebhookUrl="https://discord.com/api/webhooks/test"
        discordEvents="session.created, session.completed"
      />,
    );
    expect(screen.getByLabelText('Webhook URL')).toHaveValue(
      'https://discord.com/api/webhooks/test',
    );
    expect(screen.getByLabelText('Events')).toHaveValue('session.created, session.completed');
  });

  it('calls onDiscordWebhookUrlChange', () => {
    const onDiscordWebhookUrlChange = vi.fn();
    render(
      <NotificationsSettings {...defaults} onDiscordWebhookUrlChange={onDiscordWebhookUrlChange} />,
    );
    fireEvent.change(screen.getByLabelText('Webhook URL'), {
      target: { value: 'https://discord.com/api/webhooks/new' },
    });
    expect(onDiscordWebhookUrlChange).toHaveBeenCalledWith('https://discord.com/api/webhooks/new');
  });

  it('calls onDiscordEventsChange', () => {
    const onDiscordEventsChange = vi.fn();
    render(<NotificationsSettings {...defaults} onDiscordEventsChange={onDiscordEventsChange} />);
    fireEvent.change(screen.getByLabelText('Events'), {
      target: { value: 'session.stale' },
    });
    expect(onDiscordEventsChange).toHaveBeenCalledWith('session.stale');
  });
});
