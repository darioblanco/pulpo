<script lang="ts">
  import { Searchbar, Chip } from 'konsta/svelte';
  import type { ListSessionsParams } from '$lib/api';

  let {
    onfilter,
    statusOptions = ['completed', 'dead'],
    providerOptions = ['claude', 'codex'],
  }: {
    onfilter: (query: ListSessionsParams) => void;
    statusOptions?: string[];
    providerOptions?: string[];
  } = $props();

  let search = $state('');
  let activeStatus: string | undefined = $state(undefined);
  let activeProvider: string | undefined = $state(undefined);

  function emitFilter() {
    onfilter({
      search: search || undefined,
      status: activeStatus,
      provider: activeProvider,
    });
  }

  function handleSearch(e: Event) {
    search = (e.target as HTMLInputElement).value;
    emitFilter();
  }

  function handleClear() {
    search = '';
    emitFilter();
  }

  function toggleStatus(s: string) {
    activeStatus = activeStatus === s ? undefined : s;
    emitFilter();
  }

  function toggleProvider(p: string) {
    activeProvider = activeProvider === p ? undefined : p;
    emitFilter();
  }
</script>

<Searchbar
  value={search}
  onInput={handleSearch}
  onClear={handleClear}
  placeholder="Search sessions..."
/>
<div class="flex gap-2 px-4 pb-2 flex-wrap">
  {#each statusOptions as s (s)}
    <Chip outline={activeStatus !== s} onClick={() => toggleStatus(s)}>{s}</Chip>
  {/each}
  {#each providerOptions as p (p)}
    <Chip outline={activeProvider !== p} onClick={() => toggleProvider(p)}>{p}</Chip>
  {/each}
</div>
