import { useState, useEffect, useCallback } from 'react';
import { AppHeader } from '@/components/layout/app-header';
import { NodeSettings } from '@/components/settings/node-settings';
import { GuardSettings } from '@/components/settings/guard-settings';
import { PeerSettings } from '@/components/settings/peer-settings';
import { Button } from '@/components/ui/button';
import { Skeleton } from '@/components/ui/skeleton';
import { getConfig, updateConfig, getPeers } from '@/api/client';
import { toast } from 'sonner';
import type { PeerInfo } from '@/api/types';

export function SettingsPage() {
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [nodeName, setNodeName] = useState('');
  const [port, setPort] = useState(7433);
  const [dataDir, setDataDir] = useState('');
  const [guardPreset, setGuardPreset] = useState('standard');
  const [peers, setPeers] = useState<PeerInfo[]>([]);

  const loadConfig = useCallback(async () => {
    try {
      setLoading(true);
      const config = await getConfig();
      setNodeName(config.node.name);
      setPort(config.node.port);
      setDataDir(config.node.data_dir);
      setGuardPreset(config.guards.preset);
      const peersResp = await getPeers();
      setPeers(peersResp.peers);
      setError(null);
    } catch {
      setError('Failed to load config');
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    loadConfig();
  }, [loadConfig]);

  async function handleSave() {
    setSaving(true);
    try {
      const result = await updateConfig({
        node_name: nodeName,
        port,
        data_dir: dataDir,
        guard_preset: guardPreset,
      });
      if (result.restart_required) {
        toast('Saved. Restart pulpod for port change to take effect.');
      } else {
        toast('Settings saved.');
      }
      setError(null);
    } catch {
      setError('Failed to save config');
    } finally {
      setSaving(false);
    }
  }

  return (
    <div data-testid="settings-page">
      <AppHeader title="Settings" />
      <div className="max-w-2xl space-y-6 sm:space-y-8 p-4 sm:p-6">
        {loading ? (
          <div data-testid="loading-skeleton" className="space-y-4">
            <Skeleton className="h-32 w-full" />
            <Skeleton className="h-16 w-full" />
            <Skeleton className="h-32 w-full" />
          </div>
        ) : error && !nodeName ? (
          <p className="text-center text-destructive">{error}</p>
        ) : (
          <>
            {error && <p className="text-sm text-destructive">{error}</p>}

            <NodeSettings
              name={nodeName}
              onNameChange={setNodeName}
              port={port}
              onPortChange={setPort}
              dataDir={dataDir}
              onDataDirChange={setDataDir}
            />

            <GuardSettings preset={guardPreset} onPresetChange={setGuardPreset} />

            <PeerSettings peers={peers} onUpdate={setPeers} />

            <Button
              data-testid="save-btn"
              className="w-full"
              onClick={handleSave}
              disabled={saving}
            >
              {saving ? 'Saving...' : 'Save'}
            </Button>
          </>
        )}
      </div>
    </div>
  );
}
