import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';

interface NodeSettingsProps {
  name: string;
  onNameChange: (name: string) => void;
  port: number;
  onPortChange: (port: number) => void;
  dataDir: string;
  onDataDirChange: (dir: string) => void;
}

export function NodeSettings({
  name,
  onNameChange,
  port,
  onPortChange,
  dataDir,
  onDataDirChange,
}: NodeSettingsProps) {
  return (
    <div data-testid="node-settings" className="space-y-4">
      <h3 className="text-sm font-semibold">Node</h3>
      <div>
        <Label htmlFor="node-name">Name</Label>
        <Input
          id="node-name"
          value={name}
          onChange={(e) => onNameChange(e.target.value)}
          placeholder="my-node"
        />
      </div>
      <div>
        <Label htmlFor="node-port">Port</Label>
        <Input
          id="node-port"
          type="number"
          value={port}
          onChange={(e) => onPortChange(parseInt(e.target.value, 10) || 0)}
          placeholder="7433"
        />
      </div>
      <div>
        <Label htmlFor="node-data-dir">Data directory</Label>
        <Input
          id="node-data-dir"
          value={dataDir}
          onChange={(e) => onDataDirChange(e.target.value)}
          placeholder="~/.pulpo/data"
        />
      </div>
    </div>
  );
}
