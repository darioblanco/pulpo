<script lang="ts">
  import { onMount } from 'svelte';
  import { Preloader } from 'konsta/svelte';
  import QRCode from 'qrcode';
  import { getPairingUrl } from '$lib/api';

  let qrSvg = $state('');
  let pairingUrl = $state('');
  let error = $state('');
  let loading = $state(true);

  onMount(async () => {
    try {
      const resp = await getPairingUrl();
      pairingUrl = resp.url;
      qrSvg = await QRCode.toString(pairingUrl, { type: 'svg', margin: 1 });
    } catch {
      error = 'Failed to generate pairing code';
    } finally {
      loading = false;
    }
  });
</script>

{#if loading}
  <div class="flex items-center justify-center p-8">
    <Preloader />
  </div>
{:else if error}
  <p class="text-red-500 text-center">{error}</p>
{:else}
  <div class="flex flex-col items-center gap-4">
    <div class="w-48 h-48">
      <!-- eslint-disable-next-line svelte/no-at-html-tags -- QR SVG from trusted qrcode library -->
      {@html qrSvg}
    </div>
    <p class="text-xs opacity-60 break-all text-center max-w-xs">{pairingUrl}</p>
  </div>
{/if}
