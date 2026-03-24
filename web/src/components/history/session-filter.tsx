import { useState } from 'react';
import { Input } from '@/components/ui/input';
import { Button } from '@/components/ui/button';

interface SessionFilterProps {
  onFilter: (query: { search?: string; statuses: Set<string> }) => void;
  statusOptions?: string[];
  defaultStatuses?: string[];
}

export function SessionFilter({
  onFilter,
  statusOptions = ['active', 'idle', 'ready', 'stopped', 'lost'],
  defaultStatuses = ['active', 'idle', 'ready'],
}: SessionFilterProps) {
  const [search, setSearch] = useState('');
  const [activeStatuses, setActiveStatuses] = useState<Set<string>>(() => new Set(defaultStatuses));

  function emit(overrides: Partial<{ search: string; statuses: Set<string> }>) {
    const s = overrides.search ?? search;
    const st = overrides.statuses ?? activeStatuses;
    onFilter({
      search: s || undefined,
      statuses: st,
    });
  }

  function toggleStatus(s: string) {
    setActiveStatuses((prev) => {
      const next = new Set(prev);
      if (next.has(s)) {
        next.delete(s);
      } else {
        next.add(s);
      }
      emit({ statuses: next });
      return next;
    });
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
            variant={activeStatuses.has(s) ? 'default' : 'outline'}
            size="xs"
            aria-pressed={activeStatuses.has(s)}
            onClick={() => toggleStatus(s)}
          >
            {s}
          </Button>
        ))}
      </div>
    </div>
  );
}
