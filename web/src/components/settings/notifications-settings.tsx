import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import { FormField } from './form-field';
import type { WebhookEndpointConfigResponse } from '@/api/types';

export type WebhookFormData = WebhookEndpointConfigResponse;

interface NotificationsSettingsProps {
  webhooks: WebhookFormData[];
  onWebhooksChange: (webhooks: WebhookFormData[]) => void;
}

export function NotificationsSettings({ webhooks, onWebhooksChange }: NotificationsSettingsProps) {
  function addWebhook() {
    onWebhooksChange([...webhooks, { name: '', url: '', events: [] }]);
  }

  function updateWebhook(index: number, field: 'name' | 'url' | 'events', value: string) {
    const updated = [...webhooks];
    if (field === 'events') {
      updated[index] = {
        ...updated[index],
        events: value
          .split(',')
          .map((e) => e.trim())
          .filter(Boolean),
      };
    } else {
      updated[index] = { ...updated[index], [field]: value };
    }
    onWebhooksChange(updated);
  }

  function removeWebhook(index: number) {
    onWebhooksChange(webhooks.filter((_, i) => i !== index));
  }

  return (
    <Card data-testid="notifications-settings">
      <CardHeader>
        <CardTitle>Notifications</CardTitle>
        <CardDescription>
          Push session events to external services. Configure each target below.
        </CardDescription>
      </CardHeader>
      <CardContent>
        <div className="grid gap-4" data-testid="webhooks-content">
          <p className="text-xs text-muted-foreground">
            Generic HTTP webhooks that POST session events as JSON.
          </p>
          {webhooks.map((wh, i) => (
            <div key={i} className="rounded-lg border p-4" data-testid={`webhook-${i}`}>
              <div className="mb-3 flex items-center justify-between">
                <span className="text-sm font-medium">Webhook {i + 1}</span>
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => removeWebhook(i)}
                  data-testid={`remove-webhook-${i}`}
                >
                  Remove
                </Button>
              </div>
              <div className="grid gap-4">
                <div className="grid grid-cols-2 gap-4 items-start">
                  <FormField label="Name" htmlFor={`webhook-name-${i}`}>
                    <Input
                      id={`webhook-name-${i}`}
                      value={wh.name}
                      onChange={(e) => updateWebhook(i, 'name', e.target.value)}
                      placeholder="ci-notifications"
                    />
                  </FormField>
                  <FormField label="URL" htmlFor={`webhook-url-${i}`}>
                    <Input
                      id={`webhook-url-${i}`}
                      value={wh.url}
                      onChange={(e) => updateWebhook(i, 'url', e.target.value)}
                      placeholder="https://example.com/webhook"
                    />
                  </FormField>
                </div>
                <FormField
                  label="Events"
                  htmlFor={`webhook-events-${i}`}
                  description="Comma-separated. Leave empty for all events."
                >
                  <Input
                    id={`webhook-events-${i}`}
                    value={wh.events.join(', ')}
                    onChange={(e) => updateWebhook(i, 'events', e.target.value)}
                    placeholder="ready, stopped, lost"
                  />
                </FormField>
              </div>
            </div>
          ))}
          <Button variant="outline" size="sm" onClick={addWebhook} data-testid="add-webhook-btn">
            Add webhook
          </Button>
        </div>
      </CardContent>
    </Card>
  );
}
