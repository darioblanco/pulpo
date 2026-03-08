import { useState } from 'react';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import { Textarea } from '@/components/ui/textarea';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { Switch } from '@/components/ui/switch';
import { Label } from '@/components/ui/label';
import { FormField } from './form-field';
import { updateRemoteConfig } from '@/api/client';
import type { InkConfig, PeerInfo } from '@/api/types';
import { ChevronDown, ChevronRight, Pencil, Plus, Send, Trash2 } from 'lucide-react';

interface InkSettingsProps {
  inks: Record<string, InkConfig>;
  onInksChange: (inks: Record<string, InkConfig>) => void;
  peers?: PeerInfo[];
}

const emptyInk: InkConfig = {
  description: null,
  provider: null,
  model: null,
  mode: null,
  unrestricted: null,
  instructions: null,
};

export function InkSettings({ inks, onInksChange, peers = [] }: InkSettingsProps) {
  const [expandedInk, setExpandedInk] = useState<string | null>(null);
  const [newInkName, setNewInkName] = useState('');
  const [pushing, setPushing] = useState(false);
  const [pushResult, setPushResult] = useState<string | null>(null);

  const sortedNames = Object.keys(inks).sort();
  const onlinePeers = peers.filter((p) => p.status === 'online');

  function addInk() {
    const name = newInkName.trim().toLowerCase().replace(/\s+/g, '-');
    if (!name || inks[name]) return;
    onInksChange({ ...inks, [name]: { ...emptyInk } });
    setNewInkName('');
    setExpandedInk(name);
  }

  function removeInk(name: string) {
    const next = { ...inks };
    delete next[name];
    onInksChange(next);
    if (expandedInk === name) setExpandedInk(null);
  }

  function updateInk(
    name: string,
    field: keyof InkConfig,
    value: string | string[] | boolean | null,
  ) {
    const ink = inks[name];
    if (!ink) return;
    const resolved = typeof value === 'boolean' ? value : value || null;
    onInksChange({ ...inks, [name]: { ...ink, [field]: resolved } });
  }

  async function pushToPeers() {
    if (onlinePeers.length === 0) return;
    setPushing(true);
    setPushResult(null);
    const results: string[] = [];
    for (const peer of onlinePeers) {
      try {
        await updateRemoteConfig(peer.address, { inks });
        results.push(`${peer.name}: ok`);
      } catch (e) {
        results.push(`${peer.name}: ${e instanceof Error ? e.message : 'failed'}`);
      }
    }
    setPushResult(results.join(', '));
    setPushing(false);
  }

  return (
    <Card data-testid="ink-settings">
      <CardHeader>
        <CardTitle>Inks</CardTitle>
        <CardDescription>
          Named roles that define what an agent does. Each ink sets a provider, mode, and
          instructions that work across all providers. Select an ink when creating a session.
        </CardDescription>
      </CardHeader>
      <CardContent className="grid gap-4">
        {sortedNames.length === 0 && (
          <p className="text-sm text-muted-foreground" data-testid="ink-empty">
            No inks configured. Add one below.
          </p>
        )}

        {sortedNames.map((name) => {
          const ink = inks[name];
          const isExpanded = expandedInk === name;
          return (
            <div key={name} className="rounded-lg border" data-testid={`ink-${name}`}>
              <button
                type="button"
                className="flex w-full items-center justify-between p-3 text-left"
                onClick={() => setExpandedInk(isExpanded ? null : name)}
                data-testid={`ink-toggle-${name}`}
              >
                <div className="flex items-center gap-2">
                  {isExpanded ? (
                    <ChevronDown className="h-4 w-4" />
                  ) : (
                    <ChevronRight className="h-4 w-4" />
                  )}
                  <span className="font-medium">{name}</span>
                  {ink.description && (
                    <span className="text-xs text-muted-foreground">{ink.description}</span>
                  )}
                </div>
                <div className="flex items-center gap-1">
                  <Pencil className="h-3 w-3 text-muted-foreground" />
                </div>
              </button>
              {isExpanded && (
                <div className="border-t p-4" data-testid={`ink-editor-${name}`}>
                  <div className="grid gap-4">
                    <FormField label="Description" htmlFor={`ink-desc-${name}`}>
                      <Input
                        id={`ink-desc-${name}`}
                        value={ink.description ?? ''}
                        onChange={(e) => updateInk(name, 'description', e.target.value)}
                        placeholder="Short description of this ink"
                      />
                    </FormField>

                    <div className="grid grid-cols-2 gap-4 sm:grid-cols-4">
                      <FormField label="Provider" htmlFor={`ink-provider-${name}`}>
                        <Select
                          value={ink.provider ?? ''}
                          onValueChange={(v) => updateInk(name, 'provider', v)}
                        >
                          <SelectTrigger id={`ink-provider-${name}`} className="w-full">
                            <SelectValue placeholder="Any" />
                          </SelectTrigger>
                          <SelectContent>
                            <SelectItem value="claude">Claude</SelectItem>
                            <SelectItem value="codex">Codex</SelectItem>
                            <SelectItem value="gemini">Gemini</SelectItem>
                            <SelectItem value="open_code">OpenCode</SelectItem>
                          </SelectContent>
                        </Select>
                      </FormField>

                      <FormField label="Model" htmlFor={`ink-model-${name}`}>
                        <Input
                          id={`ink-model-${name}`}
                          value={ink.model ?? ''}
                          onChange={(e) => updateInk(name, 'model', e.target.value)}
                          placeholder="Default"
                        />
                      </FormField>

                      <FormField label="Mode" htmlFor={`ink-mode-${name}`}>
                        <Select
                          value={ink.mode ?? ''}
                          onValueChange={(v) => updateInk(name, 'mode', v)}
                        >
                          <SelectTrigger id={`ink-mode-${name}`} className="w-full">
                            <SelectValue placeholder="Any" />
                          </SelectTrigger>
                          <SelectContent>
                            <SelectItem value="interactive">Interactive</SelectItem>
                            <SelectItem value="autonomous">Autonomous</SelectItem>
                          </SelectContent>
                        </Select>
                      </FormField>

                      <div className="flex items-center gap-2 self-end pb-1">
                        <Switch
                          id={`ink-unrestricted-${name}`}
                          checked={ink.unrestricted === true}
                          onCheckedChange={(checked) => updateInk(name, 'unrestricted', checked)}
                          data-testid={`ink-unrestricted-${name}`}
                        />
                        <Label htmlFor={`ink-unrestricted-${name}`} className="text-sm">
                          Unrestricted
                        </Label>
                      </div>
                    </div>

                    <FormField label="Instructions" htmlFor={`ink-prompt-${name}`}>
                      <Textarea
                        id={`ink-prompt-${name}`}
                        rows={3}
                        value={ink.instructions ?? ''}
                        onChange={(e) => updateInk(name, 'instructions', e.target.value)}
                        placeholder="Role instructions for the agent (universal across providers)"
                      />
                    </FormField>

                    <div className="flex justify-end">
                      <Button
                        variant="destructive"
                        size="sm"
                        onClick={() => removeInk(name)}
                        data-testid={`ink-remove-${name}`}
                      >
                        <Trash2 className="mr-1 h-3 w-3" />
                        Remove
                      </Button>
                    </div>
                  </div>
                </div>
              )}
            </div>
          );
        })}

        <div className="flex gap-2">
          <Input
            value={newInkName}
            onChange={(e) => setNewInkName(e.target.value)}
            placeholder="new-ink-name"
            data-testid="ink-new-name"
            onKeyDown={(e) => {
              if (e.key === 'Enter') {
                e.preventDefault();
                addInk();
              }
            }}
          />
          <Button
            variant="outline"
            size="sm"
            onClick={addInk}
            disabled={!newInkName.trim()}
            data-testid="ink-add-btn"
          >
            <Plus className="mr-1 h-3 w-3" />
            Add
          </Button>
        </div>

        {onlinePeers.length > 0 && sortedNames.length > 0 && (
          <div className="border-t pt-4">
            <div className="flex items-center justify-between">
              <div>
                <p className="text-sm font-medium">Push to peers</p>
                <p className="text-xs text-muted-foreground">
                  Send current inks to {onlinePeers.length} online{' '}
                  {onlinePeers.length === 1 ? 'peer' : 'peers'}
                </p>
              </div>
              <Button
                variant="outline"
                size="sm"
                onClick={pushToPeers}
                disabled={pushing}
                data-testid="ink-push-btn"
              >
                <Send className="mr-1 h-3 w-3" />
                {pushing ? 'Pushing...' : 'Push'}
              </Button>
            </div>
            {pushResult && (
              <p className="mt-2 text-xs text-muted-foreground" data-testid="ink-push-result">
                {pushResult}
              </p>
            )}
          </div>
        )}
      </CardContent>
    </Card>
  );
}
