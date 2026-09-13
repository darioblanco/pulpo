import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent, within } from '@testing-library/react';
import { NotificationsSettings } from './notifications-settings';
import type { WebhookFormData } from './notifications-settings';

const defaults = {
  webhooks: [] as WebhookFormData[],
  onWebhooksChange: vi.fn(),
};

describe('NotificationsSettings', () => {
  it('renders the webhooks section', () => {
    render(<NotificationsSettings {...defaults} />);
    expect(screen.getByTestId('notifications-settings')).toBeInTheDocument();
    expect(screen.getByTestId('webhooks-content')).toBeInTheDocument();
    expect(screen.getByTestId('add-webhook-btn')).toBeInTheDocument();
  });

  it('adds a webhook', () => {
    const onWebhooksChange = vi.fn();
    render(<NotificationsSettings {...defaults} onWebhooksChange={onWebhooksChange} />);
    fireEvent.click(screen.getByTestId('add-webhook-btn'));
    expect(onWebhooksChange).toHaveBeenCalledWith([{ name: '', url: '', events: [] }]);
  });

  it('removes a webhook', () => {
    const onWebhooksChange = vi.fn();
    render(
      <NotificationsSettings
        {...defaults}
        webhooks={[
          { name: 'hook-1', url: 'https://a.com', events: [] },
          { name: 'hook-2', url: 'https://b.com', events: [] },
        ]}
        onWebhooksChange={onWebhooksChange}
      />,
    );
    fireEvent.click(screen.getByTestId('remove-webhook-0'));
    expect(onWebhooksChange).toHaveBeenCalledWith([
      { name: 'hook-2', url: 'https://b.com', events: [] },
    ]);
  });

  it('updates webhook name', () => {
    const onWebhooksChange = vi.fn();
    render(
      <NotificationsSettings
        {...defaults}
        webhooks={[{ name: '', url: '', events: [] }]}
        onWebhooksChange={onWebhooksChange}
      />,
    );
    const webhookSection = screen.getByTestId('webhook-0');
    fireEvent.change(within(webhookSection).getByLabelText('Name'), {
      target: { value: 'my-hook' },
    });
    expect(onWebhooksChange).toHaveBeenCalledWith([{ name: 'my-hook', url: '', events: [] }]);
  });

  it('updates webhook url', () => {
    const onWebhooksChange = vi.fn();
    render(
      <NotificationsSettings
        {...defaults}
        webhooks={[{ name: 'hook', url: '', events: [] }]}
        onWebhooksChange={onWebhooksChange}
      />,
    );
    const webhookSection = screen.getByTestId('webhook-0');
    fireEvent.change(within(webhookSection).getByLabelText('URL'), {
      target: { value: 'https://example.com' },
    });
    expect(onWebhooksChange).toHaveBeenCalledWith([
      { name: 'hook', url: 'https://example.com', events: [] },
    ]);
  });

  it('updates webhook events', () => {
    const onWebhooksChange = vi.fn();
    render(
      <NotificationsSettings
        {...defaults}
        webhooks={[{ name: 'hook', url: 'https://a.com', events: [] }]}
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
      },
    ]);
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
          },
        ]}
      />,
    );
    expect(screen.getByTestId('webhook-0')).toBeInTheDocument();
    expect(screen.getByDisplayValue('ci-hook')).toBeInTheDocument();
    expect(screen.getByDisplayValue('https://ci.example.com')).toBeInTheDocument();
  });
});
