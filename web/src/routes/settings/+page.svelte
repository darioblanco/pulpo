<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import {
    Page,
    Navbar,
    Block,
    BlockTitle,
    List,
    ListInput,
    Button,
    Preloader,
    Segmented,
    SegmentedButton,
    Toast,
  } from 'konsta/svelte';
  import {
    getConfig,
    updateConfig,
    getPeers,
    addPeer,
    removePeer,
    type ConfigResponse,
    type PeerInfo,
  } from '$lib/api';
  import PairingQrCode from '$lib/components/PairingQrCode.svelte';

  let config: ConfigResponse | null = $state(null);
  let loading = $state(true);
  let saving = $state(false);
  let error: string | null = $state(null);
  let toastOpen = $state(false);
  let toastMessage = $state('');

  // Form state
  let nodeName = $state('');
  let port = $state(7433);
  let dataDir = $state('');
  let guardPreset = $state('standard');

  // Peers state
  let peers: PeerInfo[] = $state([]);
  let newPeerName = $state('');
  let newPeerAddress = $state('');

  let showPairing = $state(false);

  const presets = ['permissive', 'standard', 'strict', 'locked'] as const;

  async function loadConfig() {
    try {
      loading = true;
      config = await getConfig();
      nodeName = config.node.name;
      port = config.node.port;
      dataDir = config.node.data_dir;
      guardPreset = config.guards.preset;
      const peersResp = await getPeers();
      peers = peersResp.peers;
      error = null;
    } catch {
      error = 'Failed to load config';
    } finally {
      loading = false;
    }
  }

  async function handleSave() {
    saving = true;
    try {
      const result = await updateConfig({
        node_name: nodeName,
        port,
        data_dir: dataDir,
        guard_preset: guardPreset,
      });
      config = result.config;
      if (result.restart_required) {
        showToast('Saved. Restart pulpod for port change to take effect.');
      } else {
        showToast('Settings saved.');
      }
      error = null;
    } catch {
      error = 'Failed to save config';
    } finally {
      saving = false;
    }
  }

  async function handleAddPeer() {
    if (!newPeerName.trim() || !newPeerAddress.trim()) return;
    try {
      const resp = await addPeer(newPeerName.trim(), newPeerAddress.trim());
      peers = resp.peers;
      newPeerName = '';
      newPeerAddress = '';
      showToast('Peer added.');
    } catch (e) {
      showToast((e as Error).message);
    }
  }

  async function handleRemovePeer(name: string) {
    try {
      await removePeer(name);
      peers = peers.filter((p) => p.name !== name);
      showToast('Peer removed.');
    } catch (e) {
      showToast((e as Error).message);
    }
  }

  let toastTimer: ReturnType<typeof setTimeout> | null = null;

  function showToast(msg: string) {
    toastMessage = msg;
    toastOpen = true;
    if (toastTimer) clearTimeout(toastTimer);
    toastTimer = setTimeout(() => {
      toastOpen = false;
    }, 3000);
  }

  onMount(() => {
    loadConfig();
  });

  onDestroy(() => {
    if (toastTimer) clearTimeout(toastTimer);
  });
</script>

<Page>
  <Navbar title="Settings" />

  {#if loading}
    <Block class="text-center">
      <Preloader />
      <p class="mt-4 text-color-text-muted">Loading config...</p>
    </Block>
  {:else if error}
    <Block class="text-center">
      <p class="text-color-danger">{error}</p>
      <Button class="mt-4" onClick={loadConfig}>Retry</Button>
    </Block>
  {:else}
    <BlockTitle>Node</BlockTitle>
    <List strongIos insetIos>
      <ListInput
        label="Name"
        type="text"
        value={nodeName}
        onInput={(e: Event) => (nodeName = (e.target as HTMLInputElement).value)}
      />
      <ListInput
        label="Port"
        type="number"
        value={String(port)}
        onInput={(e: Event) => (port = parseInt((e.target as HTMLInputElement).value, 10) || 0)}
      />
      <ListInput
        label="Data directory"
        type="text"
        value={dataDir}
        onInput={(e: Event) => (dataDir = (e.target as HTMLInputElement).value)}
      />
    </List>

    <BlockTitle>Guard Preset</BlockTitle>
    <Block>
      <Segmented strong>
        {#each presets as preset (preset)}
          <SegmentedButton active={guardPreset === preset} onClick={() => (guardPreset = preset)}>
            {preset}
          </SegmentedButton>
        {/each}
      </Segmented>
    </Block>

    <BlockTitle>Peers</BlockTitle>
    <List strongIos insetIos>
      {#each peers as peer (peer.name)}
        <li class="flex items-center justify-between px-4 py-2">
          <div class="flex items-center gap-2">
            <span
              class="w-2 h-2 rounded-full"
              class:bg-green-500={peer.status === 'online'}
              class:bg-gray-400={peer.status !== 'online'}
            ></span>
            <span class="font-medium">{peer.name}</span>
            <span class="text-xs text-[var(--color-text-muted)]">{peer.address}</span>
          </div>
          <Button small outline onClick={() => handleRemovePeer(peer.name)}>Remove</Button>
        </li>
      {/each}
    </List>
    <Block>
      <div class="flex gap-2 items-end">
        <div class="flex-1">
          <label for="peer-name" class="block text-xs text-[var(--color-text-muted)] mb-1"
            >Name</label
          >
          <input
            id="peer-name"
            type="text"
            class="w-full px-2 py-1 bg-transparent border border-[var(--color-border)] rounded text-sm"
            bind:value={newPeerName}
            placeholder="remote-node"
          />
        </div>
        <div class="flex-1">
          <label for="peer-address" class="block text-xs text-[var(--color-text-muted)] mb-1"
            >Address</label
          >
          <input
            id="peer-address"
            type="text"
            class="w-full px-2 py-1 bg-transparent border border-[var(--color-border)] rounded text-sm"
            bind:value={newPeerAddress}
            placeholder="10.0.0.1:7433"
          />
        </div>
        <Button small onClick={handleAddPeer}>Add</Button>
      </div>
    </Block>

    <BlockTitle>Device Pairing</BlockTitle>
    <Block>
      {#if showPairing}
        <PairingQrCode />
        <div class="mt-4 text-center">
          <Button small outline onClick={() => (showPairing = false)}>Hide QR Code</Button>
        </div>
      {:else}
        <Button onClick={() => (showPairing = true)}>Pair Device</Button>
      {/if}
    </Block>

    <Block class="pb-20">
      <Button large onClick={handleSave} disabled={saving}>
        {saving ? 'Saving...' : 'Save'}
      </Button>
    </Block>
  {/if}

  <Toast position="center" opened={toastOpen}>
    <div class="shrink">{toastMessage}</div>
  </Toast>
</Page>
