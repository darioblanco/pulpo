<script lang="ts">
  import '../app.css';
  import { page } from '$app/state';
  import { App, Tabbar, TabbarLink, Toast } from 'konsta/svelte';
  import { getToastMessage, isToastVisible } from '$lib/stores/notifications.svelte';
  import {
    getBaseUrl,
    getAuthToken,
    setAuthToken,
    loadSavedConnections,
  } from '$lib/stores/connection.svelte';
  import { onMount } from 'svelte';
  let { children } = $props();

  let activeTab = $derived(
    page.url.pathname.startsWith('/settings')
      ? 'settings'
      : page.url.pathname.startsWith('/history')
        ? 'history'
        : 'dashboard',
  );

  let showTabbar = $derived(!page.url.pathname.startsWith('/connect'));

  onMount(async () => {
    loadSavedConnections();
    // When served locally (no remote base URL) and no token yet, auto-discover
    if (!getBaseUrl() && !getAuthToken()) {
      try {
        const res = await fetch('/api/v1/auth/token');
        if (res.ok) {
          const data = await res.json();
          if (data.token) {
            setAuthToken(data.token);
          }
        }
      } catch {
        // Silently ignore — auth may not be required
      }
    }
  });
</script>

<App theme="ios" dark safeAreas>
  {@render children()}

  <Toast position="center" opened={isToastVisible()}>
    {getToastMessage()}
  </Toast>

  {#if showTabbar}
    <Tabbar labels class="left-0 bottom-0 fixed">
      <TabbarLink active={activeTab === 'dashboard'} label="Dashboard" linkProps={{ href: '/' }} />
      <TabbarLink
        active={activeTab === 'history'}
        label="History"
        linkProps={{ href: '/history' }}
      />
      <TabbarLink
        active={activeTab === 'settings'}
        label="Settings"
        linkProps={{ href: '/settings' }}
      />
    </Tabbar>
  {/if}
</App>
