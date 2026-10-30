<script lang="ts">
  // DJINN GO: your own listening, apart from the party. The song files
  // come as they are and this device plays them.
  import { onMount } from 'svelte'
  import AccountPanels from '../lib/AccountPanels.svelte'
  import { api, ApiError } from '../lib/api'
  import Icon from '../lib/Icon.svelte'
  import { param, route } from '../lib/router.svelte'
  import { app } from '../lib/state.svelte'
  import Toasts from '../lib/Toasts.svelte'
  import TwoStepPanel from '../lib/TwoStepPanel.svelte'
  import Login from '../screens/Login.svelte'
  import Signup from '../screens/Signup.svelte'
  import AddToPlaylist from './AddToPlaylist.svelte'
  import { loadLikes, player, setSaveData, stop } from './player.svelte'
  import PhoneApps from './PhoneApps.svelte'
  import PlayerBar from './PlayerBar.svelte'
  import SongMenu from './SongMenu.svelte'
  import Album from './views/Album.svelte'
  import Genre from './views/Genre.svelte'
  import Artist from './views/Artist.svelte'
  import Home from './views/Home.svelte'
  import Library from './views/Library.svelte'
  import Playlist from './views/Playlist.svelte'
  import Search from './views/Search.svelte'
  import Submit from './views/Submit.svelte'

  let ready = $state(false)
  let signingUp = $state(location.pathname === '/signup')
  let error = $state('')

  onMount(async () => {
    await refresh()
    ready = true
  })

  async function refresh() {
    try {
      app.me = await api.me()
      error = ''
    } catch (e) {
      app.me = null
      if (e instanceof ApiError && e.status !== 401) error = e.message
      app.mode = await api.mode().catch(() => null)
    }
    if (canListen()) loadLikes().catch(() => {})
  }

  const canListen = () =>
    !!app.me && !!app.me.permissions?.go && !(app.me.two_step_required && !app.me.two_step)

  async function signedIn() {
    signingUp = false
    if (location.pathname === '/signup') history.replaceState(null, '', '/')
    await refresh()
  }

  async function signOut() {
    stop()
    await api.logout().catch(() => {})
    app.me = null
    app.mode = await api.mode().catch(() => null)
  }

  const albumId = $derived(param('/album'))
  const artistId = $derived(param('/artist'))
  const playlistId = $derived(param('/playlist'))
  const genreName = $derived.by(() => {
    const m = route.path.match(/^\/genre\/(.+)$/)
    return m ? decodeURIComponent(m[1]) : null
  })
  const section = $derived(
    route.path.startsWith('/search')
      ? 'search'
      : route.path.startsWith('/library') || route.path === '/submit' || playlistId
        ? 'library'
        : route.path.startsWith('/account')
          ? 'account'
          : albumId || artistId || genreName
            ? 'search'
            : 'home',
  )
  const librarySection = $derived(
    route.path === '/library/playlists' ? 'playlists' : route.path === '/library/history' ? 'history' : 'liked',
  )
</script>

<Toasts />
{#if !ready}
  <div class="center-screen muted">Loading...</div>
{:else if !app.me}
  <main class="narrow">
    {#if signingUp}
      <Signup onback={() => (signingUp = false)} ondone={signedIn} />
    {:else}
      <Login title="DJINN GO" subtitle="Your music, apart from the party." onsignedin={signedIn} />
      {#if app.mode && app.mode.signup !== 'closed'}
        <button class="text-button" onclick={() => (signingUp = true)}>New here? Create an account</button>
      {/if}
    {/if}
    {#if error}<p class="error">{error}</p>{/if}
  </main>
{:else if !app.me.permissions?.go}
  <main class="narrow">
    <div class="card">
      <h1>DJINN GO</h1>
      <p>Your account can't use DJINN GO. Ask the host to allow it.</p>
      <button onclick={signOut}>Sign out</button>
    </div>
  </main>
{:else if app.me.two_step_required && !app.me.two_step}
  <main class="narrow">
    <h1>DJINN GO</h1>
    <p class="muted">Your account needs two-step sign-in for DJINN GO. Set it up once, then you're in.</p>
    <TwoStepPanel onchange={refresh} />
    <button class="text-button" onclick={signOut}>Sign out</button>
  </main>
{:else}
  <header>
    <a class="brand" href="#/">DJINN GO</a>
    <nav>
      <a href="#/" class:active={section === 'home'} aria-label="Home"><Icon name="home" /><span>Home</span></a>
      <a href="#/search" class:active={section === 'search'} aria-label="Library"><Icon name="search" /><span>Library</span></a>
      <a href="#/library" class:active={section === 'library'} aria-label="Your music">
        <Icon name="heart-outline" /><span>Your music</span>
      </a>
      <a href="#/account" class:active={section === 'account'} aria-label="Account"><Icon name="settings" /><span>Account</span></a>
    </nav>
  </header>
  <main class="go" class:playing={player.index >= 0}>
    {#if genreName}
      <Genre name={genreName} />
    {:else if albumId}
      <Album id={albumId} />
    {:else if artistId}
      <Artist id={artistId} />
    {:else if playlistId}
      <Playlist id={playlistId} />
    {:else if route.path === '/submit'}
      <Submit />
    {:else if section === 'search'}
      <Search />
    {:else if section === 'library'}
      <Library section={librarySection} />
    {:else if section === 'account'}
      <h1>Your account</h1>
      <p class="muted">Signed in as <b>{app.me.username}</b>{#if app.me.role_name} ({app.me.role_name}){/if}.</p>
      {#if player.canConvert}
        <section class="card">
          <h2>This device</h2>
          <label class="switch">
            <input type="checkbox" checked={player.saveData} onchange={(e) => setSaveData(e.currentTarget.checked)} />
            Save data: play smaller MP3 copies on this device
          </label>
        </section>
      {/if}
      <AccountPanels />
      <PhoneApps />
      <button class="signout" onclick={signOut}>Sign out</button>
    {:else}
      <Home />
    {/if}
  </main>
  <PlayerBar />
  <SongMenu />
  <AddToPlaylist />
{/if}

<style>
  header {
    position: sticky;
    top: 0;
    z-index: 10;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    max-width: 760px;
    margin: 0 auto;
    padding: calc(10px + env(safe-area-inset-top)) 16px 10px;
    background: var(--bg);
  }

  .brand {
    color: var(--text);
    font-weight: 700;
    text-decoration: none;
  }

  nav {
    display: flex;
    gap: 4px;
  }

  nav a {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 8px 10px;
    border-radius: 999px;
    color: var(--muted);
    text-decoration: none;
  }

  nav a.active {
    background: var(--surface-2);
    color: var(--text);
  }

  /* Icons only on narrow phones. */
  @media (max-width: 520px) {
    nav a span {
      display: none;
    }
  }

  .go {
    max-width: 760px;
    margin: 0 auto;
    padding: 4px 16px 24px;
  }

  .go.playing {
    padding-bottom: 96px;
  }

  .text-button {
    display: block;
    margin: 8px auto 0;
    border: none;
    background: transparent;
    color: var(--muted);
    text-decoration: underline;
  }

  .signout {
    width: 100%;
    margin: 8px 0 24px;
  }
</style>
