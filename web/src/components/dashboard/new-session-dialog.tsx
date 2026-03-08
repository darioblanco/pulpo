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
import { Switch } from '@/components/ui/switch';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from '@/components/ui/dialog';
import { Plus } from 'lucide-react';
import { createSession, createRemoteSession, getInks } from '@/api/client';
import type { InkConfig, PeerInfo, Session } from '@/api/types';
import { getProviderCapabilities } from '@/api/types';

interface NewSessionDialogProps {
  peers?: PeerInfo[];
  onCreated: (session: Session) => void;
}

export function NewSessionDialog({ peers = [], onCreated }: NewSessionDialogProps) {
  const [open, setOpen] = useState(false);
  const [name, setName] = useState('');
  const [repoPath, setRepoPath] = useState('');
  const [prompt, setPrompt] = useState('');
  const [provider, setProvider] = useState('claude');
  const [mode, setMode] = useState('interactive');
  const [unrestricted, setUnrestricted] = useState(false);
  const [targetNode, setTargetNode] = useState('local');
  const [selectedInk, setSelectedInk] = useState('');
  const [inks, setInks] = useState<Record<string, InkConfig>>({});
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const onlinePeers = peers.filter((p) => p.status === 'online');

  useEffect(() => {
    if (open) {
      getInks()
        .then((res) => setInks(res.inks))
        .catch(() => {
          /* inks are optional */
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
    if (ink.provider) setProvider(ink.provider);
    if (ink.mode) setMode(ink.mode);
    if (ink.unrestricted != null) setUnrestricted(ink.unrestricted);
  }

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (!repoPath || !prompt) return;
    setSubmitting(true);
    setError(null);
    try {
      const data = {
        ...(name.trim() ? { name: name.trim() } : {}),
        workdir: repoPath,
        prompt,
        provider,
        mode,
        ...(unrestricted ? { unrestricted: true } : {}),
        ...(selectedInk ? { ink: selectedInk } : {}),
      };

      let resp;
      if (targetNode === 'local') {
        resp = await createSession(data);
      } else {
        const peer = peers.find((p) => p.name === targetNode);
        if (!peer) return;
        resp = await createRemoteSession(peer.address, data);
      }

      setName('');
      setRepoPath('');
      setPrompt('');
      setSelectedInk('');
      setOpen(false);
      onCreated(resp.session);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to create session');
    } finally {
      setSubmitting(false);
    }
  }

  const inkNames = Object.keys(inks).sort();
  const caps = getProviderCapabilities(provider);
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
              placeholder="my-session (optional)"
              value={name}
              onChange={(e) => setName(e.target.value)}
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
            <Label htmlFor="prompt">Prompt</Label>
            <Textarea
              id="prompt"
              placeholder="Describe the task..."
              rows={3}
              value={prompt}
              onChange={(e) => setPrompt(e.target.value)}
              required
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
                  {[
                    activeInk.provider,
                    activeInk.mode,
                    activeInk.unrestricted ? 'unrestricted' : null,
                    activeInk.instructions
                      ? activeInk.instructions.length > 60
                        ? `${activeInk.instructions.slice(0, 60)}...`
                        : activeInk.instructions
                      : null,
                  ]
                    .filter(Boolean)
                    .join(' · ')}
                </p>
              )}
            </div>
          )}

          <div className="grid grid-cols-2 gap-3 sm:grid-cols-3">
            <div className="space-y-1.5">
              <Label htmlFor="provider">Provider</Label>
              <Select value={provider} onValueChange={setProvider}>
                <SelectTrigger id="provider" className="w-full">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="claude">Claude</SelectItem>
                  <SelectItem value="codex">Codex</SelectItem>
                  <SelectItem value="gemini">Gemini</SelectItem>
                  <SelectItem value="open_code">OpenCode</SelectItem>
                </SelectContent>
              </Select>
            </div>

            <div className="space-y-1.5">
              <Label htmlFor="mode">Mode</Label>
              <Select value={mode} onValueChange={setMode}>
                <SelectTrigger id="mode" className="w-full">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="interactive">Interactive</SelectItem>
                  <SelectItem value="autonomous">Autonomous</SelectItem>
                </SelectContent>
              </Select>
            </div>

            {caps.unrestricted && (
              <div className="flex items-center gap-2 self-end pb-1.5">
                <Switch
                  id="unrestricted-toggle"
                  checked={unrestricted}
                  onCheckedChange={setUnrestricted}
                  data-testid="unrestricted-toggle"
                />
                <Label htmlFor="unrestricted-toggle" className="text-sm">
                  Unrestricted
                </Label>
              </div>
            )}

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
          </div>

          <Button
            type="submit"
            className="w-full mt-1"
            disabled={submitting || !repoPath || !prompt}
          >
            {submitting ? 'Creating...' : 'Create Session'}
          </Button>
        </form>
      </DialogContent>
    </Dialog>
  );
}
