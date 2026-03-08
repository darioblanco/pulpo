import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Switch } from '@/components/ui/switch';
import { Label } from '@/components/ui/label';

interface GuardSettingsProps {
  unrestricted: boolean;
  onUnrestrictedChange: (val: boolean) => void;
}

export function GuardSettings({ unrestricted, onUnrestrictedChange }: GuardSettingsProps) {
  return (
    <Card data-testid="guard-settings">
      <CardHeader>
        <CardTitle>Guards</CardTitle>
        <CardDescription>
          Default safety limits applied to new sessions. Per-session overrides take precedence.
        </CardDescription>
      </CardHeader>
      <CardContent className="grid gap-4">
        <div className="flex items-center justify-between">
          <div className="space-y-0.5">
            <Label htmlFor="unrestricted-mode">Unrestricted mode</Label>
            <p className="text-xs text-muted-foreground">
              When enabled, sessions run without safety guardrails. Use with caution.
            </p>
          </div>
          <Switch
            id="unrestricted-mode"
            checked={unrestricted}
            onCheckedChange={onUnrestrictedChange}
            data-testid="guard-unrestricted-toggle"
          />
        </div>
      </CardContent>
    </Card>
  );
}
