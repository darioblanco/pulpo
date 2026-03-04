<script lang="ts">
  import { Block, Button } from 'konsta/svelte';
  import { createSession, createRemoteSession, type PeerInfo } from '$lib/api';

  let { oncreated, peers = [] }: { oncreated: () => void; peers?: PeerInfo[] } = $props();

  let repoPath = $state('');
  let prompt = $state('');
  let provider = $state('claude');
  let mode = $state('interactive');
  let guardPreset = $state('standard');
  let targetNode = $state('local');
  let submitting = $state(false);
  let error: string | null = $state(null);

  let onlinePeers = $derived(peers.filter((p) => p.status === 'online'));

  async function handleSubmit() {
    if (!repoPath || !prompt) return;
    submitting = true;
    error = null;
    try {
      const data = {
        workdir: repoPath,
        prompt,
        provider,
        mode,
        guard_preset: guardPreset,
      };

      if (targetNode === 'local') {
        await createSession(data);
      } else {
        const peer = peers.find((p) => p.name === targetNode);
        if (peer) {
          await createRemoteSession(peer.address, data);
        }
      }

      repoPath = '';
      prompt = '';
      oncreated();
    } catch (e) {
      error = e instanceof Error ? e.message : 'Failed to create session';
    } finally {
      submitting = false;
    }
  }
</script>

<Block strong inset class="!my-2">
  <form onsubmit={handleSubmit}>
    {#if error}
      <p class="text-sm text-red-500 mb-2">{error}</p>
    {/if}

    <div class="mb-3">
      <label
        for="repo-path"
        class="block text-xs font-semibold uppercase tracking-wide text-[var(--color-text-muted)] mb-1"
        >Working directory</label
      >
      <input
        id="repo-path"
        type="text"
        bind:value={repoPath}
        placeholder="/path/to/repo"
        required
        class="w-full px-3 py-2 text-sm rounded-lg border border-[var(--color-border)] bg-[var(--color-bg)] text-[var(--color-text)]"
      />
    </div>

    <div class="mb-3">
      <label
        for="prompt"
        class="block text-xs font-semibold uppercase tracking-wide text-[var(--color-text-muted)] mb-1"
        >Prompt</label
      >
      <textarea
        id="prompt"
        bind:value={prompt}
        placeholder="Describe the task..."
        rows="3"
        required
        class="w-full px-3 py-2 text-sm rounded-lg border border-[var(--color-border)] bg-[var(--color-bg)] text-[var(--color-text)] font-[inherit]"
      ></textarea>
    </div>

    <div class="flex gap-3 mb-3">
      <div class="flex-1">
        <label
          for="provider"
          class="block text-xs font-semibold uppercase tracking-wide text-[var(--color-text-muted)] mb-1"
          >Provider</label
        >
        <select
          id="provider"
          bind:value={provider}
          class="w-full px-2 py-2 text-sm rounded-lg border border-[var(--color-border)] bg-[var(--color-bg)] text-[var(--color-text)]"
        >
          <option value="claude">Claude</option>
          <option value="codex">Codex</option>
        </select>
      </div>

      <div class="flex-1">
        <label
          for="mode"
          class="block text-xs font-semibold uppercase tracking-wide text-[var(--color-text-muted)] mb-1"
          >Mode</label
        >
        <select
          id="mode"
          bind:value={mode}
          class="w-full px-2 py-2 text-sm rounded-lg border border-[var(--color-border)] bg-[var(--color-bg)] text-[var(--color-text)]"
        >
          <option value="interactive">Interactive</option>
          <option value="autonomous">Autonomous</option>
        </select>
      </div>

      <div class="flex-1">
        <label
          for="guard-preset"
          class="block text-xs font-semibold uppercase tracking-wide text-[var(--color-text-muted)] mb-1"
          >Guards</label
        >
        <select
          id="guard-preset"
          bind:value={guardPreset}
          class="w-full px-2 py-2 text-sm rounded-lg border border-[var(--color-border)] bg-[var(--color-bg)] text-[var(--color-text)]"
        >
          <option value="standard">Standard</option>
          <option value="strict">Strict</option>
          <option value="unrestricted">Unrestricted</option>
        </select>
      </div>

      <div class="flex-1">
        <label
          for="target-node"
          class="block text-xs font-semibold uppercase tracking-wide text-[var(--color-text-muted)] mb-1"
          >Node</label
        >
        <select
          id="target-node"
          bind:value={targetNode}
          class="w-full px-2 py-2 text-sm rounded-lg border border-[var(--color-border)] bg-[var(--color-bg)] text-[var(--color-text)]"
        >
          <option value="local">Local</option>
          {#each onlinePeers as peer (peer.name)}
            <option value={peer.name}>{peer.name}</option>
          {/each}
        </select>
      </div>
    </div>

    <Button
      large
      component="button"
      type="submit"
      disabled={submitting || !repoPath || !prompt}
      colors={{ fillBgIos: 'bg-[var(--color-accent)]', fillTextIos: 'text-[var(--color-bg)]' }}
    >
      {submitting ? 'Creating...' : 'Create Session'}
    </Button>
  </form>
</Block>
