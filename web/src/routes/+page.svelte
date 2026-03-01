<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { goto } from '$app/navigation';
  import { Page, Navbar, Block, Preloader, Fab } from 'konsta/svelte';
  import {
    getPeers,
    getSessions,
    getRemoteSessions,
    type NodeInfo,
    type PeerInfo,
    type Session,
  } from '$lib/api';
  import NewSessionForm from '$lib/components/NewSessionForm.svelte';
  import NodeCard from '$lib/components/NodeCard.svelte';
  import { detectStatusChanges, showDesktopNotification } from '$lib/notifications';
  import { showToast } from '$lib/stores/notifications.svelte';
  import { isConnected } from '$lib/stores/connection.svelte';

  let localNode: NodeInfo | null = $state(null);
  let peers: PeerInfo[] = $state([]);
  let localSessions: Session[] = $state([]);
  let peerSessions: Record<string, Session[]> = $state({});
  let error: string | null = $state(null);
  let showForm = $state(false);
  let polling: ReturnType<typeof setInterval> | null = null;
  let previousSessions: Session[] = [];

  async function fetchData() {
    try {
      const peersResp = await getPeers();
      localNode = peersResp.local;
      peers = peersResp.peers;

      // Fetch all sessions (including completed/dead) for change detection
      const allSessions: Session[] = await getSessions();

      // Detect status changes and fire notifications
      if (previousSessions.length > 0) {
        const changes = detectStatusChanges(previousSessions, allSessions);
        for (const change of changes) {
          const label =
            change.to === 'completed' ? 'completed' : change.to === 'dead' ? 'died' : 'resumed';
          showToast(`${change.sessionName} ${label}`);
          showDesktopNotification(change);
        }
      }
      previousSessions = allSessions;

      // Filter to active sessions for display
      localSessions = allSessions.filter(
        (s) => s.status === 'creating' || s.status === 'running' || s.status === 'stale',
      );

      // Fan out session fetches to online peers
      const peerResults: Record<string, Session[]> = {};
      const promises = peers
        .filter((p) => p.status === 'online')
        .map(async (peer) => {
          try {
            const sessions = await getRemoteSessions(peer.address);
            peerResults[peer.name] = sessions;
          } catch {
            peerResults[peer.name] = [];
          }
        });

      await Promise.all(promises);
      peerSessions = peerResults;
      error = null;
    } catch {
      // On mobile (no local pulpod), redirect to connection screen
      if (!isConnected()) {
        // eslint-disable-next-line svelte/no-navigation-without-resolve
        goto('/connect');
        return;
      }
      error = 'Failed to connect to pulpod';
    }
  }

  function handleCreated() {
    showForm = false;
    fetchData();
  }

  onMount(() => {
    fetchData();
    polling = setInterval(fetchData, 5000);
  });

  onDestroy(() => {
    if (polling) clearInterval(polling);
  });
</script>

<Page>
  <Navbar title="pulpo" />

  {#if error}
    <Block class="text-center">
      <p class="text-color-danger">{error}</p>
    </Block>
  {:else if localNode}
    <div class="px-4">
      {#if showForm}
        <NewSessionForm oncreated={handleCreated} {peers} />
      {/if}

      <!-- Local node -->
      <NodeCard
        name={localNode.name}
        nodeInfo={localNode}
        status="online"
        sessions={localSessions}
        isLocal={true}
        onrefresh={fetchData}
      />

      <!-- Peer nodes -->
      {#each peers as peer (peer.name)}
        <NodeCard
          name={peer.name}
          nodeInfo={peer.node_info}
          status={peer.status}
          sessions={peerSessions[peer.name] || []}
          onrefresh={fetchData}
        />
      {/each}
    </div>

    <Fab
      class="fixed right-4-safe bottom-4-safe z-20"
      onClick={() => (showForm = !showForm)}
      text={showForm ? 'Cancel' : '+ New Session'}
    />
  {:else}
    <Block class="text-center">
      <Preloader />
      <p class="mt-4 text-color-text-muted">Connecting to pulpod...</p>
    </Block>
  {/if}
</Page>
