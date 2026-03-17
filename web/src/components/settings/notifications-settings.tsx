import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import { Badge } from '@/components/ui/badge';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs';
import { Separator } from '@/components/ui/separator';
import { FormField } from './form-field';
import type { WebhookEndpointConfigResponse } from '@/api/types';

export interface WebhookFormData extends WebhookEndpointConfigResponse {
  secret: string;
}

interface NotificationsSettingsProps {
  discordWebhookUrl: string;
  onDiscordWebhookUrlChange: (url: string) => void;
  discordEvents: string;
  onDiscordEventsChange: (events: string) => void;
  webhooks: WebhookFormData[];
  onWebhooksChange: (webhooks: WebhookFormData[]) => void;
}

export function NotificationsSettings({
  discordWebhookUrl,
  onDiscordWebhookUrlChange,
  discordEvents,
  onDiscordEventsChange,
  webhooks,
  onWebhooksChange,
}: NotificationsSettingsProps) {
  const discordActive = discordWebhookUrl.trim().length > 0;
  const webhooksActive = webhooks.length > 0;

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
        <Tabs defaultValue="webhooks" data-testid="notifications-tabs">
          <TabsList className="mb-4 w-full">
            <TabsTrigger value="webhooks" className="flex-1 gap-2" data-testid="tab-webhooks">
              Webhooks
              <Badge variant={webhooksActive ? 'default' : 'outline'} className="text-[10px]">
                {webhooksActive ? 'active' : 'inactive'}
              </Badge>
            </TabsTrigger>
            <TabsTrigger value="discord" className="flex-1 gap-2" data-testid="tab-discord">
              Discord
              <Badge variant={discordActive ? 'default' : 'outline'} className="text-[10px]">
                {discordActive ? 'active' : 'inactive'}
              </Badge>
            </TabsTrigger>
          </TabsList>

          <TabsContent
            value="webhooks"
            data-testid="webhooks-content"
            forceMount
            className="data-[state=inactive]:hidden"
          >
            <div className="grid gap-4">
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
                        placeholder="ready, killed, lost"
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
              <Button
                variant="outline"
                size="sm"
                onClick={addWebhook}
                data-testid="add-webhook-btn"
              >
                Add webhook
              </Button>
            </div>
          </TabsContent>

          <TabsContent
            value="discord"
            data-testid="discord-content"
            forceMount
            className="data-[state=inactive]:hidden"
          >
            <div className="grid gap-4">
              <p className="text-xs text-muted-foreground">
                Send session lifecycle events to a Discord channel via webhook.
              </p>
              <Separator />
              <FormField
                label="Webhook URL"
                htmlFor="discord-webhook-url"
                description="Create a webhook in your Discord channel settings."
              >
                <Input
                  id="discord-webhook-url"
                  value={discordWebhookUrl}
                  onChange={(e) => onDiscordWebhookUrlChange(e.target.value)}
                  placeholder="https://discord.com/api/webhooks/..."
                />
              </FormField>
              <FormField
                label="Events"
                htmlFor="discord-events"
                description="Comma-separated list: session.created, session.ready, session.lost, session.killed, session.intervention"
              >
                <Input
                  id="discord-events"
                  value={discordEvents}
                  onChange={(e) => onDiscordEventsChange(e.target.value)}
                  placeholder="session.created, session.ready"
                />
              </FormField>
            </div>
          </TabsContent>
        </Tabs>
      </CardContent>
    </Card>
  );
}
