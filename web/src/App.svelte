<script lang="ts">
  import { onMount } from 'svelte'
  import Icon from './lib/Icon.svelte'
  import JoinCard from './lib/JoinCard.svelte'
  import { param, route } from './lib/router.svelte'
  import Sheet from './lib/Sheet.svelte'
  import { app, connect, isAdmin, refreshMe, refreshMode } from './lib/state.svelte'
  import Toasts from './lib/Toasts.svelte'
  import Admin from './screens/Admin.svelte'
  import { adminTabFor } from './lib/adminTabs'
  import AlbumView from './screens/AlbumView.svelte'
  import ArtistView from './screens/ArtistView.svelte'
  import GuestJoin from './screens/GuestJoin.svelte'
  import Join from './screens/Join.svelte'
  import Login from './screens/Login.svelte'
  import NowPlaying from './screens/NowPlaying.svelte'
  import Playlists from './screens/Playlists.svelte'
  import PlaylistView from './screens/PlaylistView.svelte'
  import Queue from './screens/Queue.svelte'
  import Search from './screens/Search.svelte'
  import Setup from './screens/Setup.svelte'
  import Genre from './screens/Genre.svelte'
  import Similar from './screens/Similar.svelte'
  import Tv from './screens/Tv.svelte'
  import SetupLibrary from './screens/SetupLibrary.svelte'

  const onSetupPage = $state(location.pathname === '/setup')
  // The join page is for guests who haven't signed in yet; the TV page is
  // public and read only.
  const onJoinPage = location.pathname === '/join'
  const onTvPage = location.pathname === '/tv'
  let showJoin = $state(false)

  onMount(async () => {
    if (onJoinPage || onTvPage) return
    const me = await refreshMe()
    if (me) connect()
    else await refreshMode()
  })

  // Signing out (or being signed out) re-checks how to get back in.
  $effect(() => {
    if (app.ready && !app.me && !onJoinPage && !onTvPage) refreshMode()
  })

  const genreName = $derived.by(() => {
    const m = route.path.match(/^\/genre\/(.+)$/)
    return m ? decodeURIComponent(m[1]) : null
  })
  const albumId = $derived(param('/album'))
  const artistId = $derived(param('/artist'))
  const playlistId = $derived(param('/playlist'))
  const similarId = $derived(param('/similar'))
  const adminTab = $derived(adminTabFor(route.path))
  const tab = $derived(
    route.path.startsWith('/search') || albumId || artistId || similarId || genreName
      ? 'search'
      : route.path.startsWith('/queue')
        ? 'queue'
        : route.path.startsWith('/playlist')
          ? 'playlists'
          : route.path.startsWith('/admin')
            ? 'admin'
            : 'now',
  )
</script>

{#if onTvPage}
  <Tv />
{:else if onJoinPage}
  <Join />
{:else if !app.ready}
  <div class="center-screen muted">Loading...</div>
{:else if app.me && app.setupLibrary}
  <main class="narrow"><SetupLibrary /></main>
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
    {#if similarId}
      <Similar id={similarId} />
    {:else if genreName}
      <Genre name={genreName} />
    {:else if albumId}
      <AlbumView id={albumId} />
    {:else if artistId}
      <ArtistView id={artistId} />
    {:else if playlistId && isAdmin()}
      <PlaylistView id={playlistId} />
    {:else if tab === 'search'}
      <Search />
    {:else if tab === 'queue'}
      <Queue />
    {:else if tab === 'playlists' && isAdmin()}
      <Playlists />
    {:else if tab === 'admin' && isAdmin()}
      <Admin tab={adminTab} />
    {:else}
      <NowPlaying />
    {/if}
  </main>

  <nav class:admin={isAdmin()}>
    <a href="#/" class:active={tab === 'now'}><Icon name="disc" /><span>Playing</span></a>
    <a href="#/search" class:active={tab === 'search'}><Icon name="search" /><span>Library</span></a>
    <a href="#/queue" class:active={tab === 'queue'}>
      <Icon name="queue" />
      <span>Queue{app.queue.length ? ` (${app.queue.length})` : ''}</span>
    </a>
    <button class:active={showJoin} onclick={() => (showJoin = true)}><Icon name="qr" /><span>Join</span></button>
    {#if isAdmin()}
      <a href="#/playlists" class:active={tab === 'playlists'}><Icon name="playlist" /><span>Playlists</span></a>
      <a href="#/admin" class:active={tab === 'admin'}><Icon name="settings" /><span>Admin</span></a>
    {/if}
  </nav>

  {#if showJoin}
    <Sheet title="Help a friend join" onclose={() => (showJoin = false)}>
      <JoinCard />
    </Sheet>
  {/if}
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
    padding-top: 10vh;
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

  nav a,
  nav button {
    flex: 1;
    max-width: 140px;
    min-height: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 2px;
    padding: 10px 0 8px;
    border-radius: 0;
    background: transparent;
    color: var(--muted);
    font-size: 0.75rem;
    text-decoration: none;
  }

  nav.admin a,
  nav.admin button {
    font-size: 0.6875rem;
  }

  nav a.active,
  nav button.active {
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
