<script lang="ts">
  import { onMount } from 'svelte';

  interface NodeInfo {
    name: string;
    hostname: string;
    os: string;
    arch: string;
    cpus: number;
    memory_mb: number;
    gpu: string | null;
  }

  interface Session {
    id: string;
    name: string;
    provider: string;
    status: string;
    prompt: string;
    created_at: string;
    output_preview: string | null;
  }

  let node: NodeInfo | null = $state(null);
  let sessions: Session[] = $state([]);
  let error: string | null = $state(null);

  onMount(async () => {
    try {
      const [nodeRes, sessionsRes] = await Promise.all([
        fetch('/api/v1/node'),
        fetch('/api/v1/sessions'),
      ]);
      node = await nodeRes.json();
      sessions = await sessionsRes.json();
    } catch {
      error = 'Failed to connect to nornd';
    }
  });
</script>

<div class="dashboard">
  {#if error}
    <div class="error">{error}</div>
  {:else if node}
    <section class="node-info">
      <div class="node-status">
        <span class="dot running"></span>
        <strong>{node.name}</strong>
        <span class="meta">{node.os} · {node.arch} · {node.cpus} cores</span>
      </div>
    </section>

    <section class="sessions">
      <div class="section-header">
        <h2>Sessions</h2>
        <button class="btn-new">+ New Session</button>
      </div>

      {#if sessions.length === 0}
        <p class="empty">No active sessions. Spawn one to get started.</p>
      {:else}
        {#each sessions as session (session.id)}
          <div class="session-card">
            <div class="session-status">
              <span class="dot {session.status}"></span>
              <strong>{session.name}</strong>
              <span class="provider">{session.provider}</span>
              <span class="status">{session.status}</span>
            </div>
            <p class="prompt">{session.prompt}</p>
          </div>
        {/each}
      {/if}
    </section>
  {:else}
    <p class="loading">Connecting to nornd...</p>
  {/if}
</div>

<style>
  .dashboard {
    max-width: 640px;
    margin: 0 auto;
  }

  .node-info {
    margin-bottom: 1.5rem;
  }

  .node-status {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }

  .meta {
    color: var(--text-muted);
    font-size: 0.875rem;
  }

  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    display: inline-block;
  }

  .dot.running {
    background: var(--accent);
  }
  .dot.completed {
    background: var(--text-muted);
  }
  .dot.dead {
    background: var(--danger);
  }
  .dot.stale {
    background: var(--warning);
  }
  .dot.creating {
    background: var(--warning);
  }

  .section-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 1rem;
  }

  .section-header h2 {
    font-size: 1rem;
    font-weight: 600;
  }

  .btn-new {
    background: var(--accent-dim);
    color: var(--accent);
    border: 1px solid var(--accent);
    border-radius: var(--radius);
    padding: 0.375rem 0.75rem;
    font-size: 0.875rem;
    cursor: pointer;
  }

  .session-card {
    background: var(--bg-surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 0.75rem 1rem;
    margin-bottom: 0.5rem;
  }

  .session-status {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    margin-bottom: 0.25rem;
  }

  .provider {
    color: var(--text-muted);
    font-size: 0.75rem;
    text-transform: uppercase;
  }

  .status {
    margin-left: auto;
    font-size: 0.75rem;
    color: var(--text-muted);
  }

  .prompt {
    font-size: 0.875rem;
    color: var(--text-muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .empty {
    color: var(--text-muted);
    text-align: center;
    padding: 2rem;
  }

  .loading {
    color: var(--text-muted);
    text-align: center;
    padding: 4rem;
  }

  .error {
    color: var(--danger);
    text-align: center;
    padding: 4rem;
  }
</style>
