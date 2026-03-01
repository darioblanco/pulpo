<script lang="ts">
  import { Button, Segmented, SegmentedButton } from 'konsta/svelte';
  import {
    killSession,
    resumeSession,
    getInterventionEvents,
    type Session,
    type InterventionEvent,
  } from '$lib/api';
  import Terminal from '$lib/components/Terminal.svelte';
  import ChatView from '$lib/components/ChatView.svelte';

  let {
    session,
    onkill,
  }: {
    session: Session;
    onkill: () => void;
  } = $props();

  let expanded = $state(false);
  let viewMode: 'chat' | 'terminal' = $state('chat');
  let interventionEvents: InterventionEvent[] = $state([]);
  let interventionsExpanded = $state(false);

  async function loadInterventions() {
    if (session.status === 'dead' && session.intervention_reason) {
      interventionEvents = await getInterventionEvents(session.id);
    }
  }

  async function handleKill() {
    await killSession(session.id);
    onkill();
  }

  async function handleResume() {
    await resumeSession(session.id);
    onkill(); // refresh parent
  }

  function toggleExpand() {
    expanded = !expanded;
  }
</script>

<div
  class="rounded-lg border border-[var(--color-border)] bg-[var(--color-bg-surface)] mb-2 overflow-hidden"
>
  <div
    data-testid="session-header"
    role="button"
    tabindex="0"
    onclick={toggleExpand}
    onkeydown={toggleExpand}
    class="px-4 py-3 cursor-pointer"
  >
    <div class="flex items-center gap-2 mb-1">
      <span
        class="w-2 h-2 rounded-full shrink-0"
        class:bg-[var(--color-accent)]={session.status === 'running'}
        class:bg-[var(--color-text-muted)]={session.status === 'completed'}
        class:bg-[var(--color-danger)]={session.status === 'dead'}
        class:bg-[var(--color-warning)]={session.status === 'stale' ||
          session.status === 'creating'}
      ></span>
      <strong>{session.name}</strong>
      <span class="text-xs uppercase text-[var(--color-text-muted)]">{session.provider}</span>
      <span
        class="text-[0.625rem] uppercase px-1.5 py-0.5 rounded-sm border border-[var(--color-border)] bg-[var(--color-bg)] text-[var(--color-text-muted)]"
        >{session.mode}</span
      >
      {#if session.guard_config}
        <span
          data-testid="guard-badge"
          class="text-[0.625rem] uppercase px-1.5 py-0.5 rounded-sm border border-[var(--color-border)] bg-[var(--color-bg)] text-[var(--color-text-muted)]"
          >{session.guard_config.shell === 'unrestricted'
            ? 'yolo'
            : session.guard_config.shell === 'none'
              ? 'strict'
              : 'standard'}</span
        >
      {/if}
      {#if session.status === 'dead' && session.intervention_reason}
        <span
          data-testid="intervention-badge"
          class="text-[0.625rem] uppercase px-1.5 py-0.5 rounded-sm border border-[var(--color-danger)] bg-[var(--color-danger)]/10 text-[var(--color-danger)]"
          >intervened</span
        >
      {/if}
      <span class="ml-auto text-xs text-[var(--color-text-muted)]">{session.status}</span>
    </div>
    <p class="text-sm text-[var(--color-text-muted)] truncate">{session.prompt}</p>
  </div>

  {#if expanded}
    <div class="px-4 pb-3 border-t border-[var(--color-border)]">
      <div class="flex justify-between text-xs text-[var(--color-text-muted)] py-2">
        <span>{session.workdir}</span>
        <span>{new Date(session.created_at).toLocaleString()}</span>
      </div>

      {#if session.status === 'running'}
        <Segmented strong class="my-2">
          <SegmentedButton active={viewMode === 'chat'} onClick={() => (viewMode = 'chat')}
            >Chat</SegmentedButton
          >
          <SegmentedButton active={viewMode === 'terminal'} onClick={() => (viewMode = 'terminal')}
            >Terminal</SegmentedButton
          >
        </Segmented>

        {#if viewMode === 'chat'}
          <ChatView sessionId={session.id} sessionStatus={session.status} />
        {:else}
          <Terminal sessionId={session.id} />
        {/if}
      {:else}
        <ChatView sessionId={session.id} sessionStatus={session.status} />
      {/if}

      {#if session.status === 'dead' && session.intervention_reason}
        <div class="mt-2 border border-[var(--color-danger)]/30 rounded-md p-3">
          <p class="text-sm text-[var(--color-danger)] font-medium mb-1">
            Intervention: {session.intervention_reason}
          </p>
          {#if session.intervention_at}
            <p class="text-xs text-[var(--color-text-muted)]">
              {new Date(session.intervention_at).toLocaleString()}
            </p>
          {/if}
          <button
            data-testid="interventions-toggle"
            class="text-xs text-[var(--color-accent)] mt-1 cursor-pointer"
            onclick={async () => {
              if (!interventionsExpanded) await loadInterventions();
              interventionsExpanded = !interventionsExpanded;
            }}
          >
            {interventionsExpanded ? 'Hide history' : 'Show history'}
          </button>
          {#if interventionsExpanded && interventionEvents.length > 0}
            <div data-testid="intervention-history" class="mt-2 space-y-1">
              {#each interventionEvents as event (event.id)}
                <div class="text-xs border-l-2 border-[var(--color-danger)]/50 pl-2">
                  <span class="text-[var(--color-text-muted)]"
                    >{new Date(event.created_at).toLocaleString()}</span
                  >
                  <span class="ml-1">{event.reason}</span>
                </div>
              {/each}
            </div>
          {/if}
        </div>
      {/if}

      <div class="flex gap-2 items-center flex-wrap mt-2">
        {#if session.status === 'running'}
          <Button
            small
            outline
            onClick={handleKill}
            colors={{
              outlineBorderIos: 'border-[var(--color-danger)]',
              textIos: 'text-[var(--color-danger)]',
            }}>Kill Session</Button
          >
        {/if}

        {#if session.status === 'stale'}
          <Button
            small
            onClick={handleResume}
            colors={{
              fillBgIos: 'bg-[var(--color-accent-dim)]',
              fillTextIos: 'text-[var(--color-accent)]',
            }}>Resume</Button
          >
          <Button
            small
            outline
            onClick={handleKill}
            colors={{
              outlineBorderIos: 'border-[var(--color-danger)]',
              textIos: 'text-[var(--color-danger)]',
            }}>Kill Session</Button
          >
        {/if}
      </div>
    </div>
  {/if}
</div>
