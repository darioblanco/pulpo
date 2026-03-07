import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import { FormField } from './form-field';

const presets = ['strict', 'standard', 'unrestricted'] as const;

interface GuardSettingsProps {
  preset: string;
  onPresetChange: (preset: string) => void;
  maxTurns: string;
  onMaxTurnsChange: (val: string) => void;
  maxBudgetUsd: string;
  onMaxBudgetUsdChange: (val: string) => void;
  outputFormat: string;
  onOutputFormatChange: (val: string) => void;
}

export function GuardSettings({
  preset,
  onPresetChange,
  maxTurns,
  onMaxTurnsChange,
  maxBudgetUsd,
  onMaxBudgetUsdChange,
  outputFormat,
  onOutputFormatChange,
}: GuardSettingsProps) {
  return (
    <Card data-testid="guard-settings">
      <CardHeader>
        <CardTitle>Guards</CardTitle>
        <CardDescription>
          Default safety limits applied to new sessions. Per-session overrides take precedence.
        </CardDescription>
      </CardHeader>
      <CardContent className="grid gap-6">
        <FormField
          label="Preset"
          description="Strict requires approval for writes. Standard allows most actions. Unrestricted has no limits."
        >
          <div className="flex gap-2">
            {presets.map((p) => (
              <Button
                key={p}
                data-testid={`guard-preset-${p}`}
                variant={preset === p ? 'default' : 'outline'}
                size="sm"
                aria-pressed={preset === p}
                onClick={() => onPresetChange(p)}
              >
                {p}
              </Button>
            ))}
          </div>
        </FormField>
        <div className="grid items-start gap-6 sm:grid-cols-3">
          <FormField
            label="Max turns"
            htmlFor="guard-max-turns"
            description="Leave empty for no limit."
          >
            <Input
              id="guard-max-turns"
              type="number"
              value={maxTurns}
              onChange={(e) => onMaxTurnsChange(e.target.value)}
              placeholder="No limit"
            />
          </FormField>
          <FormField
            label="Max budget (USD)"
            htmlFor="guard-max-budget"
            description="Leave empty for no limit."
          >
            <Input
              id="guard-max-budget"
              type="number"
              step="0.01"
              value={maxBudgetUsd}
              onChange={(e) => onMaxBudgetUsdChange(e.target.value)}
              placeholder="No limit"
            />
          </FormField>
          <FormField
            label="Output format"
            htmlFor="guard-output-format"
            description="e.g. json, text"
          >
            <Input
              id="guard-output-format"
              value={outputFormat}
              onChange={(e) => onOutputFormatChange(e.target.value)}
              placeholder="Default"
            />
          </FormField>
        </div>
      </CardContent>
    </Card>
  );
}
