import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Input } from '@/components/ui/input';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { FormField } from './form-field';

const bindModes = ['local', 'tailscale', 'public', 'container'] as const;

const bindDescriptions: Record<string, string> = {
  local: 'Binds to 127.0.0.1. Only reachable from this machine. No discovery, no auth.',
  tailscale: 'Binds to the Tailscale IP. Peers discovered via Tailscale API.',
  public: 'Binds to 0.0.0.0. Requires auth token. Peers via mDNS or seed.',
  container: 'Binds to 0.0.0.0. Trusts container network isolation (no auth).',
};

interface NodeSettingsProps {
  name: string;
  onNameChange: (name: string) => void;
  port: number;
  onPortChange: (port: number) => void;
  dataDir: string;
  onDataDirChange: (dir: string) => void;
  bind: string;
  onBindChange: (bind: string) => void;
  tag: string;
  onTagChange: (tag: string) => void;
  seed: string;
  onSeedChange: (seed: string) => void;
  discoveryInterval: number;
  onDiscoveryIntervalChange: (secs: number) => void;
}

export function NodeSettings({
  name,
  onNameChange,
  port,
  onPortChange,
  dataDir,
  onDataDirChange,
  bind,
  onBindChange,
  tag,
  onTagChange,
  seed,
  onSeedChange,
  discoveryInterval,
  onDiscoveryIntervalChange,
}: NodeSettingsProps) {
  const showNetworking = bind !== 'local';
  const showSeed = bind === 'public' || bind === 'container';

  return (
    <Card data-testid="node-settings">
      <CardHeader>
        <CardTitle>Node</CardTitle>
        <CardDescription>Identity, network, and discovery settings for this node.</CardDescription>
      </CardHeader>
      <CardContent className="grid gap-6">
        <div className="grid items-start gap-6 sm:grid-cols-2">
          <FormField
            label="Name"
            htmlFor="node-name"
            description="A unique identifier for this node across your fleet."
          >
            <Input
              id="node-name"
              value={name}
              onChange={(e) => onNameChange(e.target.value)}
              placeholder="my-node"
            />
          </FormField>
          <FormField
            label="Port"
            htmlFor="node-port"
            description="Changing the port requires a restart."
          >
            <Input
              id="node-port"
              type="number"
              value={port}
              onChange={(e) => onPortChange(parseInt(e.target.value, 10) || 0)}
              placeholder="7433"
            />
          </FormField>
        </div>
        <FormField
          label="Data directory"
          htmlFor="node-data-dir"
          description="Path where pulpod stores its SQLite database and session logs."
        >
          <Input
            id="node-data-dir"
            value={dataDir}
            onChange={(e) => onDataDirChange(e.target.value)}
            placeholder="~/.pulpo/data"
          />
        </FormField>
        <div className="grid items-start gap-6 sm:grid-cols-2">
          <FormField
            label="Bind mode"
            htmlFor="node-bind"
            description={bindDescriptions[bind]}
          >
            <Select value={bind} onValueChange={onBindChange}>
              <SelectTrigger data-testid="bind-mode-trigger" id="node-bind" className="w-full">
                <SelectValue placeholder="Select bind mode" />
              </SelectTrigger>
              <SelectContent>
                {bindModes.map((m) => (
                  <SelectItem key={m} value={m} data-testid={`bind-mode-${m}`}>
                    {m}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </FormField>
          <FormField
            label="Tag"
            htmlFor="node-tag"
            description="Optional label for scheduling (e.g. gpu, fast)."
          >
            <Input
              id="node-tag"
              value={tag}
              onChange={(e) => onTagChange(e.target.value)}
              placeholder="gpu"
            />
          </FormField>
        </div>
        {showNetworking && (
          <div className="grid items-start gap-6 sm:grid-cols-2">
            {showSeed && (
              <FormField
                label="Seed peer"
                htmlFor="node-seed"
                description="Address of an existing node to bootstrap discovery."
              >
                <Input
                  id="node-seed"
                  value={seed}
                  onChange={(e) => onSeedChange(e.target.value)}
                  placeholder="10.0.0.1:7433"
                />
              </FormField>
            )}
            <FormField
              label="Discovery interval"
              htmlFor="node-discovery-interval"
              description="How often to re-scan for peers (seconds)."
            >
              <Input
                id="node-discovery-interval"
                type="number"
                value={discoveryInterval}
                onChange={(e) => onDiscoveryIntervalChange(parseInt(e.target.value, 10) || 0)}
                placeholder="60"
              />
            </FormField>
          </div>
        )}
      </CardContent>
    </Card>
  );
}
