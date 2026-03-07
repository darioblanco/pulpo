import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Input } from '@/components/ui/input';
import { Separator } from '@/components/ui/separator';
import { FormField } from './form-field';

interface NotificationsSettingsProps {
  discordWebhookUrl: string;
  onDiscordWebhookUrlChange: (url: string) => void;
  discordEvents: string;
  onDiscordEventsChange: (events: string) => void;
}

export function NotificationsSettings({
  discordWebhookUrl,
  onDiscordWebhookUrlChange,
  discordEvents,
  onDiscordEventsChange,
}: NotificationsSettingsProps) {
  return (
    <Card data-testid="notifications-settings">
      <CardHeader>
        <CardTitle>Notifications</CardTitle>
        <CardDescription>
          Push session events to external services. Configure each target below.
        </CardDescription>
      </CardHeader>
      <CardContent className="grid gap-6">
        <div className="grid gap-1">
          <h4 className="text-sm font-medium">Discord</h4>
          <p className="text-xs text-muted-foreground">
            Send session lifecycle events to a Discord channel via webhook.
          </p>
        </div>
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
          description="Comma-separated list: session.created, session.completed, session.stale, session.dead, session.intervention"
        >
          <Input
            id="discord-events"
            value={discordEvents}
            onChange={(e) => onDiscordEventsChange(e.target.value)}
            placeholder="session.created, session.completed"
          />
        </FormField>
      </CardContent>
    </Card>
  );
}
