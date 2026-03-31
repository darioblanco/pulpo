import { useState, useEffect } from 'react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Textarea } from '@/components/ui/textarea';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { createSchedule, updateSchedule, getInks } from '@/api/client';
import { describeCron, isValidCron, CRON_PRESETS } from '@/lib/cron';
import { toast } from 'sonner';
import type { ScheduleInfo, InkConfig } from '@/api/types';

interface ScheduleDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  schedule?: ScheduleInfo | null;
  onSaved: () => void;
}

export function ScheduleDialog({ open, onOpenChange, schedule, onSaved }: ScheduleDialogProps) {
  const isEdit = !!schedule;

  const [name, setName] = useState('');
  const [cronPreset, setCronPreset] = useState('custom');
  const [cronExpr, setCronExpr] = useState('');
  const [command, setCommand] = useState('');
  const [workdir, setWorkdir] = useState('');
  const [selectedInk, setSelectedInk] = useState('');
  const [description, setDescription] = useState('');
  const [inks, setInks] = useState<Record<string, InkConfig>>({});
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Load inks when dialog opens
  useEffect(() => {
    if (open) {
      getInks()
        .then((res) => setInks(res.inks))
        .catch(() => {
          /* inks are optional */
        });
    }
  }, [open]);

  // Populate form when editing or reset when creating
  useEffect(() => {
    if (open && schedule) {
      setName(schedule.name);
      setCronExpr(schedule.cron);
      setCommand(schedule.command || '');
      setWorkdir(schedule.workdir || '');
      setSelectedInk(schedule.ink || '');
      setDescription(schedule.description || '');
      // Check if cron matches a preset
      const preset = CRON_PRESETS.find((p) => p.value === schedule.cron);
      setCronPreset(preset ? preset.value : 'custom');
    } else if (open) {
      setName('');
      setCronPreset('custom');
      setCronExpr('');
      setCommand('');
      setWorkdir('');
      setSelectedInk('');
      setDescription('');
      setError(null);
    }
  }, [open, schedule]);

  function handlePresetChange(value: string) {
    setCronPreset(value);
    if (value !== 'custom') {
      setCronExpr(value);
    }
  }

  function handleInkChange(inkName: string) {
    if (inkName === 'none') {
      setSelectedInk('');
      return;
    }
    setSelectedInk(inkName);
    const ink = inks[inkName];
    if (ink?.command && !command) {
      setCommand(ink.command);
    }
  }

  function validate(): string | null {
    if (!name.trim()) return 'Name is required';
    if (!cronExpr.trim()) return 'Cron expression is required';
    if (!isValidCron(cronExpr)) return 'Invalid cron expression (must be 5 fields)';
    if (!command.trim() && !selectedInk) return 'Command or ink is required';
    return null;
  }

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    const validationError = validate();
    if (validationError) {
      setError(validationError);
      return;
    }

    setSubmitting(true);
    setError(null);

    try {
      const data = {
        name: name.trim(),
        cron: cronExpr.trim(),
        workdir: workdir.trim() || '.',
        ...(command.trim() ? { command: command.trim() } : {}),
        ...(selectedInk ? { ink: selectedInk } : {}),
        ...(description.trim() ? { description: description.trim() } : {}),
      };

      if (isEdit && schedule) {
        await updateSchedule(schedule.id, data);
        toast.success(`Updated schedule "${name}"`);
      } else {
        await createSchedule(data);
        toast.success(`Created schedule "${name}"`);
      }

      onOpenChange(false);
      onSaved();
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to save schedule');
    } finally {
      setSubmitting(false);
    }
  }

  const inkNames = Object.keys(inks).sort();

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{isEdit ? 'Edit Schedule' : 'New Schedule'}</DialogTitle>
        </DialogHeader>
        <form onSubmit={handleSubmit} className="space-y-3" data-testid="schedule-form">
          {error && (
            <p className="text-sm text-destructive" data-testid="schedule-form-error">
              {error}
            </p>
          )}

          <div className="space-y-1.5">
            <Label htmlFor="schedule-name">Name</Label>
            <Input
              id="schedule-name"
              placeholder="nightly-review"
              value={name}
              onChange={(e) => setName(e.target.value)}
              required
              data-testid="schedule-name-input"
            />
          </div>

          <div className="space-y-1.5">
            <Label>Cron Schedule</Label>
            <Select value={cronPreset} onValueChange={handlePresetChange}>
              <SelectTrigger className="w-full" data-testid="cron-preset-select">
                <SelectValue placeholder="Select a preset..." />
              </SelectTrigger>
              <SelectContent>
                {CRON_PRESETS.map((p) => (
                  <SelectItem key={p.value} value={p.value}>
                    {p.label}
                  </SelectItem>
                ))}
                <SelectItem value="custom">Custom</SelectItem>
              </SelectContent>
            </Select>
            <Input
              placeholder="0 3 * * *"
              value={cronExpr}
              onChange={(e) => {
                setCronExpr(e.target.value);
                setCronPreset('custom');
              }}
              className="font-mono text-sm"
              data-testid="cron-expr-input"
            />
            {cronExpr.trim() === '' ? (
              <p className="text-xs text-muted-foreground" data-testid="cron-hint">
                minute hour day month weekday
              </p>
            ) : isValidCron(cronExpr) ? (
              <p
                className="text-xs text-green-600 dark:text-green-400"
                data-testid="cron-description"
              >
                {describeCron(cronExpr)}
              </p>
            ) : (
              <p className="text-xs text-destructive" data-testid="cron-invalid">
                Invalid cron expression (must be 5 fields)
              </p>
            )}
          </div>

          <div className="space-y-1.5">
            <Label htmlFor="schedule-command">Command</Label>
            <Input
              id="schedule-command"
              placeholder="claude -p 'review code'"
              value={command}
              onChange={(e) => setCommand(e.target.value)}
              data-testid="schedule-command-input"
            />
          </div>

          <div className="space-y-1.5">
            <Label htmlFor="schedule-workdir">Working Directory</Label>
            <Input
              id="schedule-workdir"
              placeholder="/path/to/repo"
              value={workdir}
              onChange={(e) => setWorkdir(e.target.value)}
              data-testid="schedule-workdir-input"
            />
          </div>

          {inkNames.length > 0 && (
            <div className="space-y-1.5">
              <Label>Ink</Label>
              <Select value={selectedInk || 'none'} onValueChange={handleInkChange}>
                <SelectTrigger className="w-full" data-testid="schedule-ink-select">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="none">None</SelectItem>
                  {inkNames.map((inkName) => (
                    <SelectItem key={inkName} value={inkName}>
                      {inkName}
                      {inks[inkName]?.description ? ` — ${inks[inkName].description}` : ''}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          )}

          <div className="space-y-1.5">
            <Label htmlFor="schedule-description">Description</Label>
            <Textarea
              id="schedule-description"
              placeholder="What this schedule does..."
              rows={2}
              value={description}
              onChange={(e) => setDescription(e.target.value)}
              data-testid="schedule-description-input"
            />
          </div>

          <Button
            type="submit"
            className="mt-1 w-full"
            disabled={submitting}
            data-testid="schedule-submit-button"
          >
            {submitting ? 'Saving...' : isEdit ? 'Update Schedule' : 'Create Schedule'}
          </Button>
        </form>
      </DialogContent>
    </Dialog>
  );
}
