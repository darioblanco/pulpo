<script lang="ts">
  import { onMount } from 'svelte';
  import { Page, Navbar, Block, Preloader, List, ListItem, Button } from 'konsta/svelte';
  import {
    getSessions,
    deleteSession,
    downloadSessionOutput,
    type Session,
    type ListSessionsParams,
  } from '$lib/api';
  import SessionFilter from '$lib/components/SessionFilter.svelte';

  let sessions: Session[] = $state([]);
  let loading = $state(true);
  let error: string | null = $state(null);
  let expandedId: string | null = $state(null);
  let currentFilter: ListSessionsParams = $state({ status: 'completed,dead' });

  async function fetchSessions() {
    try {
      sessions = await getSessions(currentFilter);
      error = null;
    } catch {
      error = 'Failed to load sessions';
    } finally {
      loading = false;
    }
  }

  function handleFilter(query: ListSessionsParams) {
    currentFilter = {
      ...query,
      status: query.status || 'completed,dead',
    };
    fetchSessions();
  }

  function toggleExpand(id: string) {
    expandedId = expandedId === id ? null : id;
  }

  async function handleDownload(session: Session) {
    const blob = await downloadSessionOutput(session.id);
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = `${session.name}.log`;
    a.click();
    URL.revokeObjectURL(url);
  }

  async function handleDelete(id: string) {
    await deleteSession(id);
    sessions = sessions.filter((s) => s.id !== id);
  }

  onMount(() => {
    fetchSessions();
  });
</script>

<Page>
  <Navbar title="History" />

  <SessionFilter onfilter={handleFilter} />

  {#if loading}
    <Block class="text-center">
      <Preloader />
    </Block>
  {:else if error}
    <Block class="text-center">
      <p class="text-color-danger">{error}</p>
    </Block>
  {:else if sessions.length === 0}
    <Block class="text-center">
      <p class="text-color-text-muted">No sessions found.</p>
    </Block>
  {:else}
    <List strong inset>
      {#each sessions as session (session.id)}
        <ListItem
          title={session.name}
          subtitle={session.prompt.length > 80
            ? session.prompt.slice(0, 80) + '...'
            : session.prompt}
          after={session.status}
          onClick={() => toggleExpand(session.id)}
        />
        {#if expandedId === session.id}
          <div class="px-6 pb-4 text-sm">
            <p><strong>Provider:</strong> {session.provider}</p>
            <p><strong>Created:</strong> {new Date(session.created_at).toLocaleString()}</p>
            <p><strong>Prompt:</strong> {session.prompt}</p>
            <div class="flex gap-2 mt-2">
              <Button small outline onClick={() => handleDownload(session)}>Download Log</Button>
              <Button small outline onClick={() => handleDelete(session.id)}>Delete</Button>
            </div>
          </div>
        {/if}
      {/each}
    </List>
  {/if}
</Page>
