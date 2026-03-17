import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Input } from '@/components/ui/input';
import { Switch } from '@/components/ui/switch';
import { Label } from '@/components/ui/label';
import { Button } from '@/components/ui/button';
import { FormField } from './form-field';

interface WatchdogSettingsProps {
  enabled: boolean;
  onEnabledChange: (enabled: boolean) => void;
  memoryThreshold: number;
  onMemoryThresholdChange: (val: number) => void;
  checkIntervalSecs: number;
  onCheckIntervalSecsChange: (val: number) => void;
  breachCount: number;
  onBreachCountChange: (val: number) => void;
  idleTimeoutSecs: number;
  onIdleTimeoutSecsChange: (val: number) => void;
  idleAction: string;
  onIdleActionChange: (val: string) => void;
  adoptTmux: boolean;
  onAdoptTmuxChange: (val: boolean) => void;
}

const idleActions = ['pause', 'kill'] as const;

export function WatchdogSettings({
  enabled,
  onEnabledChange,
  memoryThreshold,
  onMemoryThresholdChange,
  checkIntervalSecs,
  onCheckIntervalSecsChange,
  breachCount,
  onBreachCountChange,
  idleTimeoutSecs,
  onIdleTimeoutSecsChange,
  idleAction,
  onIdleActionChange,
  adoptTmux,
  onAdoptTmuxChange,
}: WatchdogSettingsProps) {
  return (
    <Card data-testid="watchdog-settings">
      <CardHeader>
        <CardTitle>Watchdog</CardTitle>
        <CardDescription>
          Monitors memory pressure and idle sessions. Automatically pauses or kills runaway agents.
        </CardDescription>
      </CardHeader>
      <CardContent className="grid gap-6">
        <div className="grid gap-2">
          <div className="flex items-center gap-3">
            <Switch
              id="watchdog-enabled"
              data-testid="watchdog-toggle"
              checked={enabled}
              onCheckedChange={onEnabledChange}
            />
            <Label htmlFor="watchdog-enabled">{enabled ? 'Enabled' : 'Disabled'}</Label>
          </div>
          <p className="text-xs text-muted-foreground">
            When enabled, the watchdog periodically checks memory and session activity.
          </p>
        </div>
        <div className="grid items-start gap-6 sm:grid-cols-2">
          <FormField
            label="Memory threshold (%)"
            htmlFor="watchdog-memory-threshold"
            description="Sessions are paused/killed when system memory exceeds this."
          >
            <Input
              id="watchdog-memory-threshold"
              type="number"
              min={1}
              max={100}
              value={memoryThreshold}
              onChange={(e) => onMemoryThresholdChange(parseInt(e.target.value, 10) || 0)}
            />
          </FormField>
          <FormField
            label="Check interval (seconds)"
            htmlFor="watchdog-check-interval"
            description="How often the watchdog checks resource usage."
          >
            <Input
              id="watchdog-check-interval"
              type="number"
              value={checkIntervalSecs}
              onChange={(e) => onCheckIntervalSecsChange(parseInt(e.target.value, 10) || 0)}
            />
          </FormField>
        </div>
        <div className="grid items-start gap-6 sm:grid-cols-2">
          <FormField
            label="Breach count"
            htmlFor="watchdog-breach-count"
            description="Consecutive threshold breaches before action is taken."
          >
            <Input
              id="watchdog-breach-count"
              type="number"
              value={breachCount}
              onChange={(e) => onBreachCountChange(parseInt(e.target.value, 10) || 0)}
            />
          </FormField>
          <FormField
            label="Idle timeout (seconds)"
            htmlFor="watchdog-idle-timeout"
            description="Sessions with no output for this duration are considered idle."
          >
            <Input
              id="watchdog-idle-timeout"
              type="number"
              value={idleTimeoutSecs}
              onChange={(e) => onIdleTimeoutSecsChange(parseInt(e.target.value, 10) || 0)}
            />
          </FormField>
        </div>
        <FormField label="Idle action" description="What to do when a session goes idle.">
          <div className="flex gap-2">
            {idleActions.map((a) => (
              <Button
                key={a}
                data-testid={`idle-action-${a}`}
                variant={idleAction === a ? 'default' : 'outline'}
                size="sm"
                aria-pressed={idleAction === a}
                onClick={() => onIdleActionChange(a)}
              >
                {a}
              </Button>
            ))}
          </div>
        </FormField>
        <div className="grid gap-2">
          <div className="flex items-center gap-3">
            <Switch
              id="watchdog-adopt-tmux"
              data-testid="adopt-tmux-toggle"
              checked={adoptTmux}
              onCheckedChange={onAdoptTmuxChange}
            />
            <Label htmlFor="watchdog-adopt-tmux">Auto-adopt tmux sessions</Label>
          </div>
          <p className="text-xs text-muted-foreground">
            Automatically discover and adopt external tmux sessions into pulpo management.
          </p>
        </div>
      </CardContent>
    </Card>
  );
}
