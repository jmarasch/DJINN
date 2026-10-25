<script lang="ts">
  import { onMount } from 'svelte'
  import Icon from './lib/Icon.svelte'
  import { param, route } from './lib/router.svelte'
  import { app, connect, isAdmin, refreshMe, refreshMode } from './lib/state.svelte'
  import Toasts from './lib/Toasts.svelte'
  import Admin from './screens/Admin.svelte'
  import AlbumView from './screens/AlbumView.svelte'
  import ArtistView from './screens/ArtistView.svelte'
  import GuestJoin from './screens/GuestJoin.svelte'
  import Join from './screens/Join.svelte'
  import Login from './screens/Login.svelte'
  import NowPlaying from './screens/NowPlaying.svelte'
  import Queue from './screens/Queue.svelte'
  import Search from './screens/Search.svelte'
  import Setup from './screens/Setup.svelte'

  const onSetupPage = $state(location.pathname === '/setup')
  // The join page is for guests who haven't signed in yet.
  const onJoinPage = location.pathname === '/join'

  onMount(async () => {
    if (onJoinPage) return
    const me = await refreshMe()
    if (me) connect()
    else await refreshMode()
  })

  // Signing out (or being signed out) re-checks how to get back in.
  $effect(() => {
    if (app.ready && !app.me && !onJoinPage) refreshMode()
  })

  const albumId = $derived(param('/album'))
  const artistId = $derived(param('/artist'))
  const tab = $derived(
    route.path.startsWith('/search') || albumId || artistId
      ? 'search'
      : route.path.startsWith('/queue')
        ? 'queue'
        : route.path.startsWith('/admin')
          ? 'admin'
          : 'now',
  )
</script>

{#if onJoinPage}
  <Join />
{:else if !app.ready}
  <div class="center-screen muted">Loading...</div>
{:else if !app.me}
  <main class="narrow">
    {#if onSetupPage}
      <Setup />
    {:else if app.mode?.guest_mode && !app.hostLogin}
      <GuestJoin />
    {:else}
      <Login />
      {#if app.mode?.guest_mode}
        <button class="back-to-guest" onclick={() => (app.hostLogin = false)}>Not the host? Join with a name</button>
      {/if}
    {/if}
  </main>
{:else}
  {#if !app.connected}
    <div class="offline">Reconnecting to the jukebox...</div>
  {/if}
  <main>
    {#if albumId}
      <AlbumView id={albumId} />
    {:else if artistId}
      <ArtistView id={artistId} />
    {:else if tab === 'search'}
      <Search />
    {:else if tab === 'queue'}
      <Queue />
    {:else if tab === 'admin' && isAdmin()}
      <Admin />
    {:else}
      <NowPlaying />
    {/if}
  </main>

  <nav>
    <a href="#/" class:active={tab === 'now'}><Icon name="disc" /><span>Playing</span></a>
    <a href="#/search" class:active={tab === 'search'}><Icon name="search" /><span>Library</span></a>
    <a href="#/queue" class:active={tab === 'queue'}>
      <Icon name="queue" />
      <span>Queue{app.queue.length ? ` (${app.queue.length})` : ''}</span>
    </a>
    {#if isAdmin()}
      <a href="#/admin" class:active={tab === 'admin'}><Icon name="settings" /><span>Admin</span></a>
    {/if}
  </nav>
{/if}

<Toasts />

<style>
  main {
    max-width: 640px;
    margin: 0 auto;
    padding: 8px 16px calc(84px + env(safe-area-inset-bottom));
  }

  main.narrow {
    max-width: 420px;
    padding-top: 12vh;
  }

  nav {
    position: fixed;
    left: 0;
    right: 0;
    bottom: 0;
    z-index: 10;
    display: flex;
    justify-content: center;
    padding-bottom: env(safe-area-inset-bottom);
    background: color-mix(in srgb, var(--surface) 92%, transparent);
    backdrop-filter: blur(12px);
    border-top: 1px solid var(--line);
  }

  nav a {
    flex: 1;
    max-width: 160px;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 2px;
    padding: 10px 0 8px;
    color: var(--muted);
    font-size: 0.75rem;
    text-decoration: none;
  }

  nav a.active {
    color: var(--accent);
  }

  .back-to-guest {
    display: block;
    margin: 8px auto 0;
    background: transparent;
    color: var(--muted);
    text-decoration: underline;
  }

  .offline {
    position: sticky;
    top: 0;
    z-index: 15;
    padding: 6px 16px;
    background: var(--bad-surface);
    text-align: center;
    font-size: 0.875rem;
  }

  .center-screen {
    min-height: 100dvh;
    display: grid;
    place-items: center;
  }
</style>
