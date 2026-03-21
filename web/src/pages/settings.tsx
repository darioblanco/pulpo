import { useState, useEffect, useCallback } from 'react';
import { AppHeader } from '@/components/layout/app-header';
import { NodeSettings } from '@/components/settings/node-settings';
import { WatchdogSettings } from '@/components/settings/watchdog-settings';
import { InkSettings } from '@/components/settings/ink-settings';
import { NotificationsSettings } from '@/components/settings/notifications-settings';
import { PeerSettings } from '@/components/settings/peer-settings';
import { SecretSettings } from '@/components/settings/secret-settings';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { Skeleton } from '@/components/ui/skeleton';
import { getConfig, updateConfig, getPeers } from '@/api/client';
import { toast } from 'sonner';
import type { InkConfig, PeerInfo, UpdateConfigRequest } from '@/api/types';
import type { WebhookFormData } from '@/components/settings/notifications-settings';

export function SettingsPage() {
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Node
  const [nodeName, setNodeName] = useState('');
  const [port, setPort] = useState(7433);
  const [dataDir, setDataDir] = useState('');
  const [bind, setBind] = useState('local');
  const [tag, setTag] = useState('');
  const [seed, setSeed] = useState('');
  const [discoveryInterval, setDiscoveryInterval] = useState(60);

  // Watchdog
  const [watchdogEnabled, setWatchdogEnabled] = useState(true);
  const [watchdogMemoryThreshold, setWatchdogMemoryThreshold] = useState(85);
  const [watchdogCheckInterval, setWatchdogCheckInterval] = useState(30);
  const [watchdogBreachCount, setWatchdogBreachCount] = useState(3);
  const [watchdogIdleTimeout, setWatchdogIdleTimeout] = useState(300);
  const [watchdogIdleAction, setWatchdogIdleAction] = useState('pause');
  const [watchdogAdoptTmux, setWatchdogAdoptTmux] = useState(true);

  // Notifications
  const [discordWebhookUrl, setDiscordWebhookUrl] = useState('');
  const [discordEvents, setDiscordEvents] = useState('');
  const [webhooks, setWebhooks] = useState<WebhookFormData[]>([]);

  // Inks
  const [inks, setInks] = useState<Record<string, InkConfig>>({});

  // Peers
  const [peers, setPeers] = useState<PeerInfo[]>([]);

  const loadConfig = useCallback(async () => {
    try {
      setLoading(true);
      const config = await getConfig();
      setNodeName(config.node.name);
      setPort(config.node.port);
      setDataDir(config.node.data_dir);
      setBind(config.node.bind);
      setTag(config.node.tag ?? '');
      setSeed(config.node.seed ?? '');
      setDiscoveryInterval(config.node.discovery_interval_secs);

      setWatchdogEnabled(config.watchdog.enabled);
      setWatchdogMemoryThreshold(config.watchdog.memory_threshold);
      setWatchdogCheckInterval(config.watchdog.check_interval_secs);
      setWatchdogBreachCount(config.watchdog.breach_count);
      setWatchdogIdleTimeout(config.watchdog.idle_timeout_secs);
      setWatchdogIdleAction(config.watchdog.idle_action);
      setWatchdogAdoptTmux(config.watchdog.adopt_tmux);

      setDiscordWebhookUrl(config.notifications.discord?.webhook_url ?? '');
      setDiscordEvents(config.notifications.discord?.events.join(', ') ?? '');
      setWebhooks((config.notifications.webhooks ?? []).map((w) => ({ ...w, secret: '' })));

      setInks(config.inks ?? {});

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
      const req: UpdateConfigRequest = {
        node_name: nodeName,
        port,
        data_dir: dataDir,
        bind,
        tag,
        seed,
        discovery_interval_secs: discoveryInterval,
        watchdog_enabled: watchdogEnabled,
        watchdog_memory_threshold: watchdogMemoryThreshold,
        watchdog_check_interval_secs: watchdogCheckInterval,
        watchdog_breach_count: watchdogBreachCount,
        watchdog_idle_timeout_secs: watchdogIdleTimeout,
        watchdog_idle_action: watchdogIdleAction,
        discord_webhook_url: discordWebhookUrl,
        webhooks: webhooks
          .filter((w) => w.name.trim() && w.url.trim())
          .map((w) => ({
            name: w.name,
            url: w.url,
            events: w.events,
            ...(w.secret ? { secret: w.secret } : {}),
          })),
      };

      if (Object.keys(inks).length > 0) {
        req.inks = inks;
      }

      if (discordEvents.trim()) {
        req.discord_events = discordEvents
          .split(',')
          .map((e) => e.trim())
          .filter(Boolean);
      }

      const result = await updateConfig(req);
      if (result.restart_required) {
        toast('Saved. Restart pulpod for network changes to take effect.');
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
      <div className="mx-auto max-w-2xl p-4 pb-12 sm:p-6">
        {loading ? (
          <div data-testid="loading-skeleton" className="space-y-4">
            <Skeleton className="h-48 w-full rounded-xl" />
            <Skeleton className="h-32 w-full rounded-xl" />
            <Skeleton className="h-32 w-full rounded-xl" />
          </div>
        ) : error && !nodeName ? (
          <p className="text-center text-destructive">{error}</p>
        ) : (
          <>
            {error && <p className="mb-4 text-sm text-destructive">{error}</p>}

            <div className="sticky top-0 z-10 -mx-4 mb-6 border-b border-border bg-background/95 px-4 py-3 backdrop-blur supports-[backdrop-filter]:bg-background/60 sm:-mx-6 sm:px-6">
              <Button
                data-testid="save-btn"
                className="w-full"
                size="lg"
                onClick={handleSave}
                disabled={saving}
              >
                {saving ? 'Saving...' : 'Save settings'}
              </Button>
            </div>

            <div className="space-y-10">
              {/* Node-specific settings */}
              <section data-testid="section-node">
                <div className="mb-4 flex items-center gap-2">
                  <h2 className="text-lg font-semibold">This node</h2>
                  <Badge variant="outline">node-specific</Badge>
                </div>
                <p className="mb-4 text-sm text-muted-foreground">
                  These settings apply only to this node. Each node in your fleet has its own
                  identity and network configuration.
                </p>
                <div className="space-y-6">
                  <SecretSettings />
                  <NodeSettings
                    name={nodeName}
                    onNameChange={setNodeName}
                    port={port}
                    onPortChange={setPort}
                    dataDir={dataDir}
                    onDataDirChange={setDataDir}
                    bind={bind}
                    onBindChange={setBind}
                    tag={tag}
                    onTagChange={setTag}
                    seed={seed}
                    onSeedChange={setSeed}
                    discoveryInterval={discoveryInterval}
                    onDiscoveryIntervalChange={setDiscoveryInterval}
                  />
                </div>
              </section>

              {/* Global settings */}
              <section data-testid="section-global">
                <div className="mb-4 flex items-center gap-2">
                  <h2 className="text-lg font-semibold">Global</h2>
                  <Badge variant="secondary">synced to all nodes</Badge>
                </div>
                <p className="mb-4 text-sm text-muted-foreground">
                  These settings are shared across all nodes. Changes here will be propagated to
                  every connected peer.
                </p>
                <div className="space-y-6">
                  <PeerSettings peers={peers} onUpdate={setPeers} bind={bind} />
                  <InkSettings inks={inks} onInksChange={setInks} peers={peers} />
                  <WatchdogSettings
                    enabled={watchdogEnabled}
                    onEnabledChange={setWatchdogEnabled}
                    memoryThreshold={watchdogMemoryThreshold}
                    onMemoryThresholdChange={setWatchdogMemoryThreshold}
                    checkIntervalSecs={watchdogCheckInterval}
                    onCheckIntervalSecsChange={setWatchdogCheckInterval}
                    breachCount={watchdogBreachCount}
                    onBreachCountChange={setWatchdogBreachCount}
                    idleTimeoutSecs={watchdogIdleTimeout}
                    onIdleTimeoutSecsChange={setWatchdogIdleTimeout}
                    idleAction={watchdogIdleAction}
                    onIdleActionChange={setWatchdogIdleAction}
                    adoptTmux={watchdogAdoptTmux}
                    onAdoptTmuxChange={setWatchdogAdoptTmux}
                  />
                  <NotificationsSettings
                    discordWebhookUrl={discordWebhookUrl}
                    onDiscordWebhookUrlChange={setDiscordWebhookUrl}
                    discordEvents={discordEvents}
                    onDiscordEventsChange={setDiscordEvents}
                    webhooks={webhooks}
                    onWebhooksChange={setWebhooks}
                  />
                </div>
              </section>
            </div>
          </>
        )}
      </div>
    </div>
  );
}
