import { useState } from 'react';
import { Input } from '@/components/ui/input';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import type { KnowledgeFilterParams } from '@/pages/knowledge';

interface KnowledgeFilterProps {
  onFilter: (params: KnowledgeFilterParams) => void;
}

export function KnowledgeFilter({ onFilter }: KnowledgeFilterProps) {
  const [kind, setKind] = useState<string>('');
  const [repo, setRepo] = useState('');
  const [ink, setInk] = useState('');

  function apply(updates: Partial<KnowledgeFilterParams>) {
    const next = {
      kind: updates.kind ?? kind,
      repo: updates.repo ?? repo,
      ink: updates.ink ?? ink,
    };
    onFilter({
      kind: next.kind || undefined,
      repo: next.repo || undefined,
      ink: next.ink || undefined,
    });
  }

  return (
    <div className="flex flex-wrap items-center gap-2" data-testid="knowledge-filter">
      <Select
        value={kind}
        onValueChange={(v) => {
          const val = v === 'all' ? '' : v;
          setKind(val);
          apply({ kind: val });
        }}
      >
        <SelectTrigger className="w-36" data-testid="kind-select">
          <SelectValue placeholder="All kinds" />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value="all">All kinds</SelectItem>
          <SelectItem value="summary">Summary</SelectItem>
          <SelectItem value="failure">Failure</SelectItem>
        </SelectContent>
      </Select>

      <Input
        placeholder="Filter by repo…"
        value={repo}
        onChange={(e) => {
          setRepo(e.target.value);
          apply({ repo: e.target.value });
        }}
        className="w-48"
        data-testid="repo-input"
      />

      <Input
        placeholder="Filter by ink…"
        value={ink}
        onChange={(e) => {
          setInk(e.target.value);
          apply({ ink: e.target.value });
        }}
        className="w-36"
        data-testid="ink-input"
      />
    </div>
  );
}
