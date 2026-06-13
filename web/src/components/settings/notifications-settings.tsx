import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import { Separator } from '@/components/ui/separator';
import { Switch } from '@/components/ui/switch';
import { Label } from '@/components/ui/label';
import { FormField } from './form-field';
import { usePushNotifications } from '@/hooks/use-push-notifications';
import type { WebhookEndpointConfigResponse } from '@/api/types';

export interface WebhookFormData extends WebhookEndpointConfigResponse {
  secret: string;
}

interface NotificationsSettingsProps {
  webhooks: WebhookFormData[];
  onWebhooksChange: (webhooks: WebhookFormData[]) => void;
}

export function NotificationsSettings({ webhooks, onWebhooksChange }: NotificationsSettingsProps) {
  const { isSupported, isEnabled, isLoading, permission, enable, disable } = usePushNotifications();

  function addWebhook() {
    onWebhooksChange([
      ...webhooks,
      { name: '', url: '', events: [], has_secret: false, secret: '' },
    ]);
  }

  function updateWebhook(
    index: number,
    field: 'name' | 'url' | 'events' | 'secret',
    value: string,
  ) {
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
        <div className="mb-4 grid gap-2" data-testid="push-section">
          <div className="flex items-center gap-3">
            <Switch
              id="push-notifications"
              data-testid="push-toggle"
              checked={isEnabled}
              disabled={!isSupported || isLoading || permission === 'denied'}
              onCheckedChange={(checked) => (checked ? enable() : disable())}
            />
            <Label htmlFor="push-notifications">
              {!isSupported
                ? 'Not supported'
                : permission === 'denied'
                  ? 'Blocked'
                  : isEnabled
                    ? 'Enabled'
                    : 'Disabled'}
            </Label>
          </div>
          <p className="text-xs text-muted-foreground">
            Receive notifications when sessions finish, even when the app is closed.
          </p>
        </div>
        <Separator className="mb-4" />
        <div className="grid gap-4" data-testid="webhooks-content">
          <p className="text-xs text-muted-foreground">
            Generic HTTP webhooks that POST session events as JSON. Add HMAC signing for
            verification.
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
                <FormField
                  label="Secret"
                  htmlFor={`webhook-secret-${i}`}
                  description={
                    wh.has_secret && !wh.secret
                      ? 'A secret is configured. Leave empty to keep it, or enter a new one to replace it.'
                      : 'Optional HMAC-SHA256 signing key. Sent as X-Pulpo-Signature header.'
                  }
                >
                  <Input
                    id={`webhook-secret-${i}`}
                    type="password"
                    value={wh.secret}
                    onChange={(e) => updateWebhook(i, 'secret', e.target.value)}
                    placeholder={wh.has_secret ? '••••••••' : 'Optional signing secret'}
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
