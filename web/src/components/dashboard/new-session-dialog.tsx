import { useState } from 'react';
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
import { Plus } from 'lucide-react';
import { createSession, createRemoteSession } from '@/api/client';
import type { PeerInfo, Session } from '@/api/types';

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
  const [guardPreset, setGuardPreset] = useState('standard');
  const [targetNode, setTargetNode] = useState('local');
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const onlinePeers = peers.filter((p) => p.status === 'online');

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
        guard_preset: guardPreset,
      };

      let session: Session;
      if (targetNode === 'local') {
        session = await createSession(data);
      } else {
        const peer = peers.find((p) => p.name === targetNode);
        if (!peer) return;
        session = await createRemoteSession(peer.address, data);
      }

      setName('');
      setRepoPath('');
      setPrompt('');
      setOpen(false);
      onCreated(session);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to create session');
    } finally {
      setSubmitting(false);
    }
  }

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
        <form onSubmit={handleSubmit} className="space-y-4">
          {error && <p className="text-sm text-destructive">{error}</p>}

          <div>
            <Label htmlFor="session-name">Name</Label>
            <Input
              id="session-name"
              placeholder="my-session (optional)"
              value={name}
              onChange={(e) => setName(e.target.value)}
            />
          </div>

          <div>
            <Label htmlFor="repo-path">Working directory</Label>
            <Input
              id="repo-path"
              placeholder="/path/to/repo"
              value={repoPath}
              onChange={(e) => setRepoPath(e.target.value)}
              required
            />
          </div>

          <div>
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

          <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
            <div>
              <Label htmlFor="provider">Provider</Label>
              <Select value={provider} onValueChange={setProvider}>
                <SelectTrigger id="provider">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="claude">Claude</SelectItem>
                  <SelectItem value="codex">Codex</SelectItem>
                </SelectContent>
              </Select>
            </div>

            <div>
              <Label htmlFor="mode">Mode</Label>
              <Select value={mode} onValueChange={setMode}>
                <SelectTrigger id="mode">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="interactive">Interactive</SelectItem>
                  <SelectItem value="autonomous">Autonomous</SelectItem>
                </SelectContent>
              </Select>
            </div>

            <div>
              <Label htmlFor="guard-preset">Guards</Label>
              <Select value={guardPreset} onValueChange={setGuardPreset}>
                <SelectTrigger id="guard-preset">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="standard">Standard</SelectItem>
                  <SelectItem value="strict">Strict</SelectItem>
                  <SelectItem value="unrestricted">Unrestricted</SelectItem>
                </SelectContent>
              </Select>
            </div>

            <div>
              <Label htmlFor="target-node">Node</Label>
              <Select value={targetNode} onValueChange={setTargetNode}>
                <SelectTrigger id="target-node">
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

          <Button type="submit" className="w-full" disabled={submitting || !repoPath || !prompt}>
            {submitting ? 'Creating...' : 'Create Session'}
          </Button>
        </form>
      </DialogContent>
    </Dialog>
  );
}
