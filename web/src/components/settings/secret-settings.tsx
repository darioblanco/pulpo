import { useState, useEffect, useCallback } from 'react';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { getSecrets, setSecret, deleteSecret } from '@/api/client';
import { toast } from 'sonner';
import type { SecretEntry } from '@/api/types';

export function SecretSettings() {
  const [secrets, setSecrets] = useState<SecretEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [newName, setNewName] = useState('');
  const [newValue, setNewValue] = useState('');
  const [newEnv, setNewEnv] = useState('');
  const [showValue, setShowValue] = useState(false);
  const [adding, setAdding] = useState(false);
  const [deletingName, setDeletingName] = useState<string | null>(null);

  const loadSecrets = useCallback(async () => {
    try {
      setLoading(true);
      const entries = await getSecrets();
      setSecrets(entries);
    } catch {
      toast.error('Failed to load secrets');
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    loadSecrets();
  }, [loadSecrets]);

  async function handleAdd() {
    if (!newName.trim() || !newValue.trim()) return;
    setAdding(true);
    try {
      await setSecret(newName.trim(), newValue.trim(), newEnv.trim() || undefined);
      toast.success(`Secret "${newName.trim()}" saved.`);
      setNewName('');
      setNewValue('');
      setNewEnv('');
      setShowValue(false);
      await loadSecrets();
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Failed to set secret');
    } finally {
      setAdding(false);
    }
  }

  async function handleDelete(name: string) {
    setDeletingName(name);
    try {
      await deleteSecret(name);
      toast.success(`Secret "${name}" deleted.`);
      await loadSecrets();
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Failed to delete secret');
    } finally {
      setDeletingName(null);
    }
  }

  return (
    <Card data-testid="secret-settings">
      <CardHeader>
        <CardTitle>Secrets</CardTitle>
        <p className="text-sm text-muted-foreground">
          Environment variables injected into sessions via <code>--secret</code> flags. Values are
          never returned by the API after saving.
        </p>
      </CardHeader>
      <CardContent className="space-y-4">
        {loading ? (
          <p className="text-sm text-muted-foreground">Loading...</p>
        ) : secrets.length === 0 ? (
          <p data-testid="secrets-empty" className="text-sm text-muted-foreground">
            No secrets configured. Add secrets to inject environment variables into sessions.
          </p>
        ) : (
          <div className="space-y-2" data-testid="secrets-list">
            {secrets.map((s) => (
              <div
                key={s.name}
                data-testid={`secret-${s.name}`}
                className="flex items-center justify-between rounded-md border p-3"
              >
                <div>
                  <p className="font-mono text-sm font-medium">{s.name}</p>
                  {s.env && s.env !== s.name && (
                    <p
                      className="font-mono text-xs text-muted-foreground"
                      data-testid={`secret-env-${s.name}`}
                    >
                      ENV: {s.env}
                    </p>
                  )}
                  <p className="text-xs text-muted-foreground">
                    Created {new Date(s.created_at).toLocaleDateString()}
                  </p>
                </div>
                <Button
                  variant="destructive"
                  size="sm"
                  data-testid={`delete-secret-${s.name}`}
                  disabled={deletingName === s.name}
                  onClick={() => handleDelete(s.name)}
                >
                  {deletingName === s.name ? 'Deleting...' : 'Delete'}
                </Button>
              </div>
            ))}
          </div>
        )}

        <div className="space-y-3 rounded-md border p-4" data-testid="add-secret-form">
          <h4 className="text-sm font-medium">Add secret</h4>
          <div className="grid gap-2">
            <Label htmlFor="secret-name">Secret name</Label>
            <Input
              id="secret-name"
              data-testid="secret-name-input"
              placeholder="GITHUB_TOKEN"
              value={newName}
              onChange={(e) => setNewName(e.target.value.toUpperCase().replace(/[^A-Z0-9_]/g, ''))}
            />
          </div>
          <div className="grid gap-2">
            <Label htmlFor="secret-env">Env var name (optional)</Label>
            <Input
              id="secret-env"
              data-testid="secret-env-input"
              placeholder="defaults to secret name"
              value={newEnv}
              onChange={(e) => setNewEnv(e.target.value.toUpperCase().replace(/[^A-Z0-9_]/g, ''))}
            />
          </div>
          <div className="grid gap-2">
            <Label htmlFor="secret-value">Secret value</Label>
            <div className="flex gap-2">
              <Input
                id="secret-value"
                data-testid="secret-value-input"
                type={showValue ? 'text' : 'password'}
                placeholder="Enter secret value"
                value={newValue}
                onChange={(e) => setNewValue(e.target.value)}
              />
              <Button
                type="button"
                variant="outline"
                size="sm"
                data-testid="toggle-value-visibility"
                onClick={() => setShowValue(!showValue)}
              >
                {showValue ? 'Hide' : 'Show'}
              </Button>
            </div>
          </div>
          <Button
            data-testid="add-secret-btn"
            disabled={adding || !newName.trim() || !newValue.trim()}
            onClick={handleAdd}
          >
            {adding ? 'Adding...' : 'Add secret'}
          </Button>
        </div>
      </CardContent>
    </Card>
  );
}
