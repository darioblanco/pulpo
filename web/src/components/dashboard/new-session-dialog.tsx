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
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from '@/components/ui/dialog';
import { Plus, GitBranch, X } from 'lucide-react';
import { Switch } from '@/components/ui/switch';
import { Badge } from '@/components/ui/badge';
import { createSession, getInks, getSecrets } from '@/api/client';
import type { InkConfig, PeerInfo, SecretEntry, Session } from '@/api/types';

interface NewSessionDialogProps {
  peers?: PeerInfo[];
  onCreated: (session: Session) => void;
}

export function NewSessionDialog({ peers = [], onCreated }: NewSessionDialogProps) {
  const [open, setOpen] = useState(false);
  const [name, setName] = useState('');
  const [repoPath, setRepoPath] = useState('');
  const [command, setCommand] = useState('');
  const [description, setDescription] = useState('');
  const [targetNode, setTargetNode] = useState('local');
  const [selectedInk, setSelectedInk] = useState('');
  const [inks, setInks] = useState<Record<string, InkConfig>>({});
  const [worktree, setWorktree] = useState(false);
  const [worktreeBase, setWorktreeBase] = useState('');
  const [runtime, setRuntime] = useState<'tmux' | 'docker'>('tmux');
  const [idleThreshold, setIdleThreshold] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [availableSecrets, setAvailableSecrets] = useState<SecretEntry[]>([]);
  const [selectedSecrets, setSelectedSecrets] = useState<string[]>([]);

  const onlinePeers = peers.filter((p) => p.status === 'online');

  useEffect(() => {
    if (open) {
      getInks()
        .then((res) => setInks(res.inks))
        .catch(() => {
          /* inks are optional */
        });
      getSecrets()
        .then((entries) => setAvailableSecrets(entries))
        .catch(() => {
          /* secrets are optional */
        });
    }
  }, [open]);

  function handleInkChange(inkName: string) {
    setSelectedInk(inkName);
    if (inkName === 'none' || !inkName) {
      setSelectedInk('');
      return;
    }
    const ink = inks[inkName];
    if (!ink) return;
    if (ink.command) setCommand(ink.command);
  }

  function toggleSecret(secretName: string) {
    setSelectedSecrets((prev) =>
      prev.includes(secretName) ? prev.filter((s) => s !== secretName) : [...prev, secretName],
    );
  }

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (!name.trim() || !repoPath) return;
    setSubmitting(true);
    setError(null);
    try {
      const data = {
        name: name.trim(),
        workdir: repoPath,
        ...(command ? { command } : {}),
        ...(description ? { description } : {}),
        ...(selectedInk ? { ink: selectedInk } : {}),
        ...(worktree ? { worktree: true } : {}),
        ...(worktree && worktreeBase ? { worktree_base: worktreeBase } : {}),
        ...(runtime !== 'tmux' ? { runtime } : {}),
        ...(idleThreshold ? { idle_threshold_secs: Number(idleThreshold) } : {}),
        ...(selectedSecrets.length > 0 ? { secrets: selectedSecrets } : {}),
      };

      const resp = await createSession(
        targetNode === 'local' ? data : { ...data, target_node: targetNode },
      );

      setName('');
      setRepoPath('');
      setCommand('');
      setDescription('');
      setSelectedInk('');
      setWorktree(false);
      setWorktreeBase('');
      setRuntime('tmux');
      setIdleThreshold('');
      setSelectedSecrets([]);
      setOpen(false);
      onCreated(resp.session);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to create session');
    } finally {
      setSubmitting(false);
    }
  }

  const inkNames = Object.keys(inks).sort();
  const activeInk = selectedInk ? inks[selectedInk] : null;

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild>
        <Button data-testid="new-session-button">
          <Plus className="mr-2 h-4 w-4" />
          New Session
        </Button>
      </DialogTrigger>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>Create New Session</DialogTitle>
        </DialogHeader>
        <form onSubmit={handleSubmit} className="space-y-3">
          {error && <p className="text-sm text-destructive">{error}</p>}

          <div className="space-y-1.5">
            <Label htmlFor="session-name">Name</Label>
            <Input
              id="session-name"
              placeholder="my-session"
              value={name}
              onChange={(e) => setName(e.target.value)}
              required
            />
          </div>

          <div className="space-y-1.5">
            <Label htmlFor="repo-path">Working directory</Label>
            <Input
              id="repo-path"
              placeholder="/path/to/repo"
              value={repoPath}
              onChange={(e) => setRepoPath(e.target.value)}
              required
            />
          </div>

          <div className="space-y-1.5">
            <div className="flex items-center gap-2">
              <Switch
                id="worktree-toggle"
                checked={worktree}
                onCheckedChange={setWorktree}
                size="sm"
              />
              <Label htmlFor="worktree-toggle" className="flex items-center gap-1 text-sm">
                <GitBranch className="h-3.5 w-3.5" />
                Worktree
              </Label>
              {worktree && (
                <span className="text-xs text-muted-foreground">
                  Run in an isolated git worktree
                </span>
              )}
            </div>
            {worktree && (
              <div className="space-y-1.5" data-testid="worktree-base-field">
                <Label htmlFor="worktree-base">Base Branch</Label>
                <Input
                  id="worktree-base"
                  placeholder="main (default: current HEAD)"
                  value={worktreeBase}
                  onChange={(e) => setWorktreeBase(e.target.value)}
                />
              </div>
            )}
          </div>

          <div className="space-y-1.5">
            <Label htmlFor="command">Command</Label>
            <Input
              id="command"
              placeholder="claude code (optional, uses ink or default)"
              value={command}
              onChange={(e) => setCommand(e.target.value)}
            />
          </div>

          <div className="space-y-1.5">
            <Label htmlFor="description">Description</Label>
            <Textarea
              id="description"
              placeholder="Describe the task (optional)..."
              rows={3}
              value={description}
              onChange={(e) => setDescription(e.target.value)}
            />
          </div>

          {inkNames.length > 0 && (
            <div className="space-y-1.5">
              <Label htmlFor="ink-select">Ink</Label>
              <Select value={selectedInk || 'none'} onValueChange={handleInkChange}>
                <SelectTrigger id="ink-select" className="w-full">
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
              {activeInk && (
                <p className="text-xs text-muted-foreground" data-testid="ink-summary">
                  {[activeInk.command, activeInk.description].filter(Boolean).join(' · ')}
                </p>
              )}
            </div>
          )}

          {availableSecrets.length > 0 && (
            <div className="space-y-1.5" data-testid="secrets-picker">
              <Label>Secrets</Label>
              <div className="flex flex-wrap gap-1.5">
                {availableSecrets.map((s) => {
                  const isSelected = selectedSecrets.includes(s.name);
                  return (
                    <Badge
                      key={s.name}
                      variant={isSelected ? 'default' : 'outline'}
                      className="cursor-pointer"
                      data-testid={`secret-badge-${s.name}`}
                      onClick={() => toggleSecret(s.name)}
                    >
                      {s.name}
                      {isSelected && <X className="ml-1 h-3 w-3" />}
                    </Badge>
                  );
                })}
              </div>
              {selectedSecrets.length > 0 && (
                <p className="text-xs text-muted-foreground" data-testid="secrets-selected-count">
                  {selectedSecrets.length} secret{selectedSecrets.length > 1 ? 's' : ''} selected
                </p>
              )}
            </div>
          )}

          <div className="grid grid-cols-2 gap-3">
            <div className="space-y-1.5">
              <Label htmlFor="target-node">Node</Label>
              <Select value={targetNode} onValueChange={setTargetNode}>
                <SelectTrigger id="target-node" className="w-full">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="local">Local</SelectItem>
                  {onlinePeers.map((peer) => (
                    <SelectItem key={peer.name} value={peer.name}>
                      {peer.name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="runtime-select">Runtime</Label>
              <Select value={runtime} onValueChange={(v) => setRuntime(v as 'tmux' | 'docker')}>
                <SelectTrigger id="runtime-select" className="w-full">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="tmux">tmux</SelectItem>
                  <SelectItem value="docker">docker</SelectItem>
                </SelectContent>
              </Select>
            </div>
          </div>

          <div className="space-y-1.5">
            <Label htmlFor="idle-threshold">Idle Threshold (seconds)</Label>
            <Input
              id="idle-threshold"
              type="number"
              placeholder="60 (default)"
              min={0}
              value={idleThreshold}
              onChange={(e) => setIdleThreshold(e.target.value)}
            />
          </div>

          <Button type="submit" className="mt-1 w-full" disabled={submitting || !repoPath}>
            {submitting ? 'Creating...' : 'Create Session'}
          </Button>
        </form>
      </DialogContent>
    </Dialog>
  );
}
