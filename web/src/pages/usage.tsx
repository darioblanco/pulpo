import { useState, useEffect, useCallback } from 'react';
import { AppHeader } from '@/components/layout/app-header';
import { Badge } from '@/components/ui/badge';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { Skeleton } from '@/components/ui/skeleton';
import { getUsageProjection } from '@/api/client';
import { toast } from 'sonner';
import { Wallet } from 'lucide-react';
import type { UsageProjectionResponse, SessionProjection, AccountRollup } from '@/api/types';

/** Compact a token count: 1234 → "1.2K", 4_500_000 → "4.5M". */
function fmtTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return String(n);
}

/** Estimated (output-scraped) costs get a `~`; exact reader-derived costs are plain. */
function fmtCost(c: number | null, exact = true): string {
  if (c == null) return '—';
  return `${exact ? '' : '~'}$${c.toFixed(2)}`;
}

function fmtRate(c: number | null): string {
  return c == null ? '—' : `$${c.toFixed(2)}/h`;
}

/** Quota column: exact Codex %, estimated Claude ~%, or "—". */
function fmtQuota(s: SessionProjection): string {
  if (s.quota_used_percent != null) return `${s.quota_used_percent.toFixed(0)}%`;
  if (s.allowance_used_percent != null) return `~${s.allowance_used_percent.toFixed(0)}%`;
  return '—';
}

function accountLabel(a: AccountRollup): string {
  return a.email ?? a.provider ?? 'unknown';
}

export function UsagePage() {
  const [data, setData] = useState<UsageProjectionResponse | null>(null);
  const [loading, setLoading] = useState(true);

  const fetchUsage = useCallback(async () => {
    try {
      setData(await getUsageProjection());
    } catch {
      toast.error('Failed to load usage');
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void fetchUsage();
  }, [fetchUsage]);

  return (
    <div data-testid="usage-page">
      <AppHeader title="Usage" />
      <div className="space-y-6 p-4 sm:p-6">
        {loading ? (
          <Skeleton className="h-40 w-full" data-testid="usage-loading" />
        ) : !data || data.sessions.length === 0 ? (
          <div className="py-12 text-center" data-testid="usage-empty">
            <Wallet className="mx-auto mb-3 h-10 w-10 text-muted-foreground" />
            <p className="text-muted-foreground">No usage data yet</p>
            <p className="mt-1 text-sm text-muted-foreground">
              Spend appears once an agent session reports tokens.
            </p>
          </div>
        ) : (
          <>
            {data.accounts.length > 0 && (
              <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3" data-testid="account-cards">
                {data.accounts.map((a) => (
                  <Card key={`${accountLabel(a)}-${a.pool}`} data-testid="account-card">
                    <CardHeader className="pb-2">
                      <CardTitle className="flex items-center justify-between text-sm font-medium">
                        <span className="truncate">{accountLabel(a)}</span>
                        <Badge variant="outline" className="text-xs">
                          {a.pool}
                        </Badge>
                      </CardTitle>
                    </CardHeader>
                    <CardContent className="space-y-1">
                      <div className="text-2xl font-semibold">
                        {fmtCost(a.total_cost_usd, a.cost_is_exact)}
                      </div>
                      <div className="text-xs text-muted-foreground">
                        {fmtTokens(a.total_tokens)} tokens · {a.session_count} session
                        {a.session_count === 1 ? '' : 's'}
                        {a.cost_per_hour != null && ` · ${fmtRate(a.cost_per_hour)}`}
                      </div>
                      {a.max_quota_used_percent != null && (
                        <div className="text-xs text-muted-foreground">
                          quota {a.max_quota_used_percent.toFixed(0)}%
                        </div>
                      )}
                    </CardContent>
                  </Card>
                ))}
              </div>
            )}

            <div className="overflow-x-auto rounded-lg border border-border">
              <table className="w-full text-sm" data-testid="usage-table">
                <thead>
                  <tr className="border-b border-border bg-muted/50 text-left text-xs text-muted-foreground">
                    <th className="px-4 py-2.5 font-medium">Session</th>
                    <th className="px-4 py-2.5 font-medium">Pool</th>
                    <th className="px-4 py-2.5 text-right font-medium">Tokens</th>
                    <th className="px-4 py-2.5 text-right font-medium">Cost</th>
                    <th className="hidden px-4 py-2.5 text-right font-medium sm:table-cell">
                      $/hr
                    </th>
                    <th className="px-4 py-2.5 text-right font-medium">Quota</th>
                  </tr>
                </thead>
                <tbody>
                  {data.sessions.map((s) => (
                    <tr
                      key={s.session_id}
                      data-testid={`usage-row-${s.session_name}`}
                      className="border-b border-border last:border-0"
                    >
                      <td className="px-4 py-3 font-medium">{s.session_name}</td>
                      <td className="px-4 py-3">
                        <Badge variant="outline" className="text-xs">
                          {s.pool}
                        </Badge>
                      </td>
                      <td className="px-4 py-3 text-right font-mono text-xs">
                        {fmtTokens(s.total_tokens)}
                      </td>
                      <td className="px-4 py-3 text-right font-mono text-xs">
                        {fmtCost(s.cost_usd, s.usage_source != null)}
                      </td>
                      <td className="hidden px-4 py-3 text-right font-mono text-xs sm:table-cell">
                        {fmtRate(s.cost_per_hour)}
                      </td>
                      <td className="px-4 py-3 text-right font-mono text-xs">{fmtQuota(s)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </>
        )}
      </div>
    </div>
  );
}
