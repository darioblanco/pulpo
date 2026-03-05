import { useState } from 'react';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';
import type { ListSessionsParams } from '@/api/types';

interface SessionFilterProps {
  onFilter: (query: ListSessionsParams) => void;
  statusOptions?: string[];
  providerOptions?: string[];
}

export function SessionFilter({
  onFilter,
  statusOptions = ['completed', 'dead'],
  providerOptions = ['claude', 'codex'],
}: SessionFilterProps) {
  const [search, setSearch] = useState('');
  const [activeStatus, setActiveStatus] = useState<string | undefined>(undefined);
  const [activeProvider, setActiveProvider] = useState<string | undefined>(undefined);

  function emit(overrides: Partial<{ search: string; status?: string; provider?: string }>) {
    const s = overrides.search ?? search;
    const st = 'status' in overrides ? overrides.status : activeStatus;
    const pr = 'provider' in overrides ? overrides.provider : activeProvider;
    onFilter({
      search: s || undefined,
      status: st,
      provider: pr,
    });
  }

  function toggleStatus(s: string) {
    const next = activeStatus === s ? undefined : s;
    setActiveStatus(next);
    emit({ status: next });
  }

  function toggleProvider(p: string) {
    const next = activeProvider === p ? undefined : p;
    setActiveProvider(next);
    emit({ provider: next });
  }

  return (
    <div data-testid="session-filter" className="space-y-3">
      <Input
        data-testid="search-input"
        placeholder="Search sessions..."
        value={search}
        onChange={(e) => {
          setSearch(e.target.value);
          emit({ search: e.target.value });
        }}
      />
      <div className="flex flex-wrap gap-2">
        {statusOptions.map((s) => (
          <Button
            key={s}
            data-testid={`status-chip-${s}`}
            variant={activeStatus === s ? 'default' : 'outline'}
            size="xs"
            aria-pressed={activeStatus === s}
            onClick={() => toggleStatus(s)}
          >
            {s}
          </Button>
        ))}
        {providerOptions.map((p) => (
          <Button
            key={p}
            data-testid={`provider-chip-${p}`}
            variant={activeProvider === p ? 'default' : 'outline'}
            size="xs"
            aria-pressed={activeProvider === p}
            onClick={() => toggleProvider(p)}
          >
            {p}
          </Button>
        ))}
      </div>
    </div>
  );
}
