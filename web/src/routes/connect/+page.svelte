<script lang="ts">
  import { goto } from '$app/navigation';
  import { page } from '$app/state';
  import { Page, Navbar, Block, Button, ListInput, Preloader } from 'konsta/svelte';
  import { testConnection } from '$lib/connection';
  import { getPeers, type PeerInfo } from '$lib/api';
  import {
    setBaseUrl,
    setAuthToken,
    addSavedConnection,
    removeSavedConnection,
    getSavedConnections,
    loadSavedConnections,
  } from '$lib/stores/connection.svelte';
  import { onMount } from 'svelte';

  let url = $state('');
  let token = $state('');
  let testing = $state(false);
  let error = $state('');
  let discoveredPeers = $state<PeerInfo[]>([]);
  let loadingPeers = $state(false);

  async function fetchDiscoveredPeers() {
    loadingPeers = true;
    try {
      const resp = await getPeers();
      discoveredPeers = resp.peers.filter(
        (p) => p.source === 'discovered' && p.status === 'online',
      );
    } catch {
      discoveredPeers = [];
    } finally {
      loadingPeers = false;
    }
  }

  onMount(() => {
    loadSavedConnections();
    const params = page.url.searchParams;
    const urlToken = params.get('token');
    if (urlToken) {
      token = urlToken;
    }
    fetchDiscoveredPeers();
    const interval = setInterval(fetchDiscoveredPeers, 10000);
    return () => clearInterval(interval);
  });

  async function handleConnect(connectUrl: string, connectToken?: string) {
    testing = true;
    error = '';
    try {
      const node = await testConnection(connectUrl, connectToken);
      setBaseUrl(connectUrl);
      if (connectToken) {
        setAuthToken(connectToken);
      }
      addSavedConnection({
        name: node.name,
        url: connectUrl,
        token: connectToken,
        lastConnected: new Date().toISOString(),
      });
      // eslint-disable-next-line svelte/no-navigation-without-resolve
      goto('/');
    } catch {
      error = `Failed to connect to ${connectUrl}`;
    } finally {
      testing = false;
    }
  }

  function handleSubmit(e: Event) {
    e.preventDefault();
    if (!url.trim()) return;
    handleConnect(url.trim(), token.trim() || undefined);
  }

  function handleRemove(connectionUrl: string) {
    removeSavedConnection(connectionUrl);
  }

  function connectToPeer(peer: PeerInfo) {
    const peerUrl = `http://${peer.address}`;
    handleConnect(peerUrl);
  }
</script>

<Page>
  <Navbar title="Connect to Pulpo" />

  {#if discoveredPeers.length > 0}
    <Block>
      <p class="mb-2 text-sm font-medium">Nearby Devices</p>
      {#each discoveredPeers as peer (peer.name)}
        <button
          class="flex w-full items-center justify-between rounded-lg bg-green-500/10 p-3 text-left mb-2"
          onclick={() => connectToPeer(peer)}
        >
          <div>
            <p class="font-medium">{peer.name}</p>
            <p class="text-sm opacity-60">{peer.address}</p>
          </div>
          <span class="text-xs text-green-500">discovered</span>
        </button>
      {/each}
    </Block>
  {/if}

  {#if loadingPeers}
    <Block>
      <div class="flex items-center gap-2">
        <Preloader />
        <span class="text-sm opacity-60">Scanning for nearby devices...</span>
      </div>
    </Block>
  {/if}

  <Block>
    <form onsubmit={handleSubmit}>
      <ListInput
        label="Server URL"
        type="url"
        placeholder="http://mac-mini:7433"
        value={url}
        oninput={(e: Event) => (url = (e.target as HTMLInputElement).value)}
      />

      <ListInput
        label="Auth Token (optional)"
        type="password"
        placeholder="Leave empty for local connections"
        value={token}
        oninput={(e: Event) => (token = (e.target as HTMLInputElement).value)}
      />

      <div class="mt-4 px-4">
        <Button onClick={() => handleSubmit(new Event('submit'))} disabled={testing || !url.trim()}>
          {#if testing}
            <Preloader />
            <span class="ml-2">Connecting...</span>
          {:else}
            Connect
          {/if}
        </Button>
      </div>
    </form>

    {#if error}
      <div class="mt-4 px-4">
        <p class="text-red-500">{error}</p>
      </div>
    {/if}
  </Block>

  {#if getSavedConnections().length > 0}
    <Block>
      <p class="mb-2 text-sm font-medium">Saved Connections</p>
      {#each getSavedConnections() as conn (conn.url)}
        <div
          class="flex items-center justify-between rounded-lg bg-black/5 p-3 dark:bg-white/5 mb-2"
        >
          <button class="text-left flex-1" onclick={() => handleConnect(conn.url, conn.token)}>
            <p class="font-medium">{conn.name}</p>
            <p class="text-sm opacity-60">{conn.url}</p>
          </button>
          <button class="ml-2 text-sm text-red-500" onclick={() => handleRemove(conn.url)}>
            Remove
          </button>
        </div>
      {/each}
    </Block>
  {/if}
</Page>
