import { Button } from '@/components/ui/button';

const presets = ['strict', 'standard', 'unrestricted'] as const;

interface GuardSettingsProps {
  preset: string;
  onPresetChange: (preset: string) => void;
}

export function GuardSettings({ preset, onPresetChange }: GuardSettingsProps) {
  return (
    <div data-testid="guard-settings" className="space-y-3">
      <h3 className="text-sm font-semibold">Guard Preset</h3>
      <div className="flex gap-2">
        {presets.map((p) => (
          <Button
            key={p}
            data-testid={`guard-preset-${p}`}
            variant={preset === p ? 'default' : 'outline'}
            size="xs"
            aria-pressed={preset === p}
            onClick={() => onPresetChange(p)}
          >
            {p}
          </Button>
        ))}
      </div>
    </div>
  );
}
