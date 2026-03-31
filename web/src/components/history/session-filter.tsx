import { useState, useEffect, useCallback } from 'react';
import { useSearchParams } from 'react-router';
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
  const [searchParams, setSearchParams] = useSearchParams();

  const [search, setSearch] = useState(() => searchParams.get('q') ?? '');
  const [activeStatuses, setActiveStatuses] = useState<Set<string>>(() => {
    const fromUrl = searchParams.get('status');
    if (fromUrl) {
      return new Set(fromUrl.split(',').filter(Boolean));
    }
    return new Set(defaultStatuses);
  });

  const syncToUrl = useCallback(
    (statuses: Set<string>, query: string) => {
      setSearchParams(
        (prev) => {
          const next = new URLSearchParams(prev);
          const statusStr = [...statuses].sort().join(',');
          const defaultStr = [...defaultStatuses].sort().join(',');
          if (statusStr === defaultStr) {
            next.delete('status');
          } else {
            next.set('status', statusStr);
          }
          if (query) {
            next.set('q', query);
          } else {
            next.delete('q');
          }
          return next;
        },
        { replace: true },
      );
    },
    [setSearchParams, defaultStatuses],
  );

  // Emit initial filter from URL on mount
  useEffect(() => {
    onFilter({
      search: search || undefined,
      statuses: activeStatuses,
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  function emit(overrides: Partial<{ search: string; statuses: Set<string> }>) {
    const s = overrides.search ?? search;
    const st = overrides.statuses ?? activeStatuses;
    onFilter({
      search: s || undefined,
      statuses: st,
    });
    syncToUrl(st, s);
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
