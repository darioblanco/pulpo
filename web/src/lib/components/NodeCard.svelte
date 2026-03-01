<script lang="ts">
  import { Card, Badge } from 'konsta/svelte';
  import type { NodeInfo, Session } from '$lib/api';
  import SessionDetail from '$lib/components/SessionDetail.svelte';

  let {
    name,
    nodeInfo,
    status,
    sessions,
    isLocal = false,
    onrefresh,
  }: {
    name: string;
    nodeInfo: NodeInfo | null;
    status: 'online' | 'offline' | 'unknown';
    sessions: Session[];
    isLocal?: boolean;
    onrefresh: () => void;
  } = $props();
</script>

<Card
  outline
  class={status === 'offline' || status === 'unknown' ? 'opacity-50' : ''}
  colors={{ outlineIos: 'border-[var(--color-border)]', bgIos: 'bg-[var(--color-bg-surface)]' }}
>
  {#snippet header()}
    <div class="flex items-center gap-2 w-full">
      <span
        class="w-2 h-2 rounded-full shrink-0"
        class:bg-[var(--color-accent)]={status === 'online'}
        class:bg-[var(--color-danger)]={status === 'offline'}
        class:bg-[var(--color-text-muted)]={status === 'unknown'}
      ></span>
      <strong>{name}</strong>
      {#if isLocal}
        <Badge
          colors={{ bg: 'bg-[var(--color-accent-dim)]', text: 'text-[var(--color-accent)]' }}
          class="!text-[0.625rem] uppercase border border-[var(--color-accent)]">local</Badge
        >
      {/if}
      {#if nodeInfo}
        <span class="text-sm text-[var(--color-text-muted)]"
          >{nodeInfo.os} · {nodeInfo.arch} · {nodeInfo.cpus} cores</span
        >
      {/if}
      <span class="ml-auto text-xs text-[var(--color-text-muted)]"
        >{sessions.length} session{sessions.length !== 1 ? 's' : ''}</span
      >
    </div>
  {/snippet}

  {#if status !== 'offline' && status !== 'unknown'}
    {#if sessions.length === 0}
      <p class="text-center text-sm text-[var(--color-text-muted)] py-4">
        No active sessions on this node.
      </p>
    {:else}
      {#each sessions as session (session.id)}
        <SessionDetail {session} onkill={onrefresh} />
      {/each}
    {/if}
  {:else}
    <p class="text-center text-sm italic text-[var(--color-text-muted)] py-4">
      Node is {status} — cannot fetch sessions.
    </p>
  {/if}
</Card>
