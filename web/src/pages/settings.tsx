import { useState, useEffect, useCallback } from 'react';
import { AppHeader } from '@/components/layout/app-header';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Skeleton } from '@/components/ui/skeleton';
import { getConfig } from '@/api/client';
import type { ConfigResponse } from '@/api/types';

/** One label/value row in the read-only configuration list. */
function ConfigRow({ label, value }: { label: string; value: React.ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-4 border-b border-border py-2 text-sm last:border-0">
      <span className="text-muted-foreground">{label}</span>
      <span className="max-w-[60%] break-all text-right font-mono text-xs">{value}</span>
    </div>
  );
}

/**
 * Read-only view of pulpod's effective configuration.
 *
 * The config file (`~/.pulpo/config.toml`) is the source of truth — the web UI only
 * reads it. To change anything, edit the file and restart pulpod.
 */
export function SettingsPage() {
  const [config, setConfig] = useState<ConfigResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const loadConfig = useCallback(async () => {
    try {
      setLoading(true);
      setConfig(await getConfig());
      setError(null);
    } catch {
      setError('Failed to load config');
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void loadConfig();
  }, [loadConfig]);

  const node = config?.node;
  const watchdog = config?.watchdog;
  const webhooks = config?.notifications?.webhooks ?? [];
  const waitingPatterns = watchdog?.extra_waiting_patterns ?? [];

  return (
    <div data-testid="settings-page">
      <AppHeader title="Configuration" />
      <div className="mx-auto max-w-2xl space-y-4 p-4 pb-12 sm:p-6">
        <p className="text-sm text-muted-foreground" data-testid="config-hint">
          This is a read-only view of pulpod&rsquo;s effective configuration. To change it, edit{' '}
          <code className="rounded bg-muted px-1 py-0.5">~/.pulpo/config.toml</code> and restart
          pulpod.
        </p>

        {loading ? (
          <div data-testid="loading-skeleton" className="space-y-4">
            <Skeleton className="h-48 w-full rounded-xl" />
            <Skeleton className="h-32 w-full rounded-xl" />
            <Skeleton className="h-32 w-full rounded-xl" />
          </div>
        ) : error ? (
          <p className="text-center text-destructive">{error}</p>
        ) : (
          <>
            <Card data-testid="section-node">
              <CardHeader>
                <CardTitle>Node</CardTitle>
                <CardDescription>Identity and network settings for this node.</CardDescription>
              </CardHeader>
              <CardContent>
                <ConfigRow label="Name" value={node?.name ?? '—'} />
                <ConfigRow label="Port" value={node?.port ?? '—'} />
                <ConfigRow label="Data directory" value={node?.data_dir ?? '—'} />
                <ConfigRow label="Bind mode" value={node?.bind ?? '—'} />
              </CardContent>
            </Card>

            <Card data-testid="section-watchdog">
              <CardHeader>
                <CardTitle>Watchdog</CardTitle>
                <CardDescription>
                  Monitors idle sessions. Automatically pauses or kills unattended agents.
                </CardDescription>
              </CardHeader>
              <CardContent>
                <ConfigRow label="Enabled" value={watchdog?.enabled ? 'Yes' : 'No'} />
                <ConfigRow
                  label="Check interval (seconds)"
                  value={watchdog?.check_interval_secs ?? '—'}
                />
                <ConfigRow
                  label="Idle timeout (seconds)"
                  value={watchdog?.idle_timeout_secs ?? '—'}
                />
                <ConfigRow label="Idle action" value={watchdog?.idle_action ?? '—'} />
                <ConfigRow
                  label="Idle threshold (seconds)"
                  value={watchdog?.idle_threshold_secs ?? '—'}
                />
                {waitingPatterns.length > 0 && (
                  <ConfigRow label="Extra waiting patterns" value={waitingPatterns.join(', ')} />
                )}
              </CardContent>
            </Card>

            <Card data-testid="section-notifications">
              <CardHeader>
                <CardTitle>Notifications</CardTitle>
                <CardDescription>Webhooks that receive session events.</CardDescription>
              </CardHeader>
              <CardContent>
                {webhooks.length === 0 ? (
                  <p className="text-sm text-muted-foreground" data-testid="no-webhooks">
                    No webhooks configured.
                  </p>
                ) : (
                  <div className="space-y-3">
                    {webhooks.map((w) => (
                      <div
                        key={w.name}
                        className="rounded-lg border border-border p-3"
                        data-testid={`webhook-${w.name}`}
                      >
                        <ConfigRow label="Name" value={w.name} />
                        <ConfigRow label="URL" value={w.url} />
                        <ConfigRow
                          label="Events"
                          value={w.events.length > 0 ? w.events.join(', ') : 'all'}
                        />
                        {w.min_severity && (
                          <ConfigRow label="Min severity" value={w.min_severity} />
                        )}
                      </div>
                    ))}
                  </div>
                )}
              </CardContent>
            </Card>
          </>
        )}
      </div>
    </div>
  );
}
