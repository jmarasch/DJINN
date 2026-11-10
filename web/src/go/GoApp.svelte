<script lang="ts">
  import { t } from '../lib/i18n.svelte'
  // DJINN GO: your own listening, apart from the party. The song files
  // come as they are and this device plays them.
  import { onMount } from 'svelte'
  import AccountPanels from '../lib/AccountPanels.svelte'
  import { api, ApiError, onUnauthorized } from '../lib/api'
  import Icon from '../lib/Icon.svelte'
  import { goBack, navigation, param, route } from '../lib/router.svelte'
  import { app } from '../lib/state.svelte'
  import Toasts from '../lib/Toasts.svelte'
  import TwoStepPanel from '../lib/TwoStepPanel.svelte'
  import Login from '../screens/Login.svelte'
  import Signup from '../screens/Signup.svelte'
  import AddToPlaylist from './AddToPlaylist.svelte'
  import { loadLikes, loadPrefs, player, prefs, setSaveData, setTap, stop } from './player.svelte'
  import GetApp from './GetApp.svelte'
  import OpenOnPhone from './OpenOnPhone.svelte'
  import PhoneApps from './PhoneApps.svelte'
  import PlayerBar from './PlayerBar.svelte'
  import NowPlaying from './views/NowPlaying.svelte'
  import SongMenu from './SongMenu.svelte'
  import Album from './views/Album.svelte'
  import Genre from './views/Genre.svelte'
  import Artist from './views/Artist.svelte'
  import ArtistSongs from './views/ArtistSongs.svelte'
  import Home from './views/Home.svelte'
  import Library from './views/Library.svelte'
  import Playlist from './views/Playlist.svelte'
  import Search from './views/Search.svelte'
  import Submit from './views/Submit.svelte'

  /** So what sticks to the top of a page can stick just below this. */
  let headerHeight = $state(0)

  let ready = $state(false)
  let signingUp = $state(location.pathname === '/signup')
  let error = $state('')

  onMount(async () => {
    onUnauthorized(checkSignedIn)
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
    if (canListen()) {
      loadLikes().catch(() => {})
      loadPrefs().catch(() => {})
    }
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

  let checking = false

  /**
   * A sign-in can end from elsewhere: an admin, another device. Saving the
   * place in the song fails within seconds when it does, and the music stops.
   */
  async function checkSignedIn() {
    if (checking || !app.me) return
    checking = true
    try {
      await api.me()
    } catch (e) {
      if (e instanceof ApiError && e.status === 401) {
        stop()
        app.me = null
        app.mode = await api.mode().catch(() => null)
      }
    } finally {
      checking = false
    }
  }

  const albumId = $derived(param('/album'))
  const artistId = $derived(param('/artist'))
  const artistSongsId = $derived.by(() => {
    const m = route.path.match(/^\/artist\/(\d+)\/songs$/)
    return m ? Number(m[1]) : null
  })
  const playlistId = $derived(param('/playlist'))
  const genreName = $derived.by(() => {
    const m = route.path.match(/^\/genre\/(.+)$/)
    return m ? decodeURIComponent(m[1]) : null
  })
  const section = $derived(
    route.path === '/playing'
      ? 'playing'
      : route.path.startsWith('/search')
      ? 'search'
      : route.path.startsWith('/library') || route.path === '/submit' || playlistId
        ? 'library'
        : route.path.startsWith('/account')
          ? 'account'
          : albumId || artistId || artistSongsId || genreName
            ? 'search'
            : 'home',
  )
  const librarySection = $derived(
    route.path === '/library/playlists'
      ? 'playlists'
      : route.path === '/library/genres'
        ? 'genres'
        : route.path === '/library/history'
          ? 'history'
          : route.path === '/library/year'
            ? 'year'
            : 'liked',
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
      <GetApp />
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
  <header bind:offsetHeight={headerHeight}>
    <a class="brand" href="#/">DJINN GO</a>
    <nav aria-label="Main navigation">
      <button class="back" disabled={!navigation.canGoBack} onclick={goBack} aria-label={t('nav.back')} title={t('nav.back')}>
        <Icon name="back" size={28} /><span>{t('nav.back')}</span>
      </button>
      <a href="#/" class:active={section === 'home'} aria-label={t('go.home')}><Icon name="home" /><span>{t('go.home')}</span></a>
      <a href="#/search" class:active={section === 'search'} aria-label={t('nav.library')}><Icon name="search" /><span>{t('nav.library')}</span></a>
      <a href="#/library" class:active={section === 'library'} aria-label={t('go.your_music')}>
        <Icon name="heart-outline" /><span>{t('go.your_music')}</span>
      </a>
      <a href="#/account" class:active={section === 'account'} aria-label={t('go.account')}><Icon name="settings" /><span>{t('go.account')}</span></a>
    </nav>
  </header>
  <main class="go" class:playing={player.index >= 0 && route.path !== '/playing'} style:--nav-height="{headerHeight}px">
    {#if route.path === '/playing'}
      <NowPlaying />
    {:else if genreName}
      <Genre name={genreName} />
    {:else if albumId}
      <Album id={albumId} />
    {:else if artistId}
      <Artist id={artistId} />
    {:else if artistSongsId}
      <ArtistSongs id={artistSongsId} />
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
      <section class="card">
        <h2>Playing</h2>
        <p class="small">When you tap a song:</p>
        <label class="switch">
          <input type="radio" name="tap" checked={prefs.tap === 'play'} onchange={() => setTap('play')} />
          Play it now
        </label>
        <p class="small">Or add it to the queue:</p>
        <label class="switch indent">
          <input type="radio" name="tap" checked={prefs.tap === 'queue'} onchange={() => setTap('queue')} />
          Play it next
        </label>
        <label class="switch indent">
          <input type="radio" name="tap" checked={prefs.tap === 'end'} onchange={() => setTap('end')} />
          Add it to the end of the queue
        </label>
        <p class="small muted">
          Play next goes after the song playing and any you lined up before, ahead of the rest of the album, playlist or
          radio. The end of the queue is after all of it. Drag songs in Up next to change the order. Starting a radio
          keeps the song playing and lines the radio up after it. This goes for every device you use GO on.
        </p>
      </section>
      {#if player.canConvert}
        <section class="card">
          <h2>This device</h2>
          <label class="switch">
            <input type="checkbox" checked={player.saveData} onchange={(e) => setSaveData(e.currentTarget.checked)} />
            Save data: play smaller MP3 copies on this device
          </label>
        </section>
      {/if}
      <GetApp />
      <OpenOnPhone />
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
    flex-direction: column;
    gap: 8px;
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

  nav a,
  nav button {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    flex: 1;
    min-width: 0;
    min-height: 44px;
    padding: 8px 10px;
    border-radius: 999px;
    color: var(--muted);
    text-decoration: none;
    background: transparent;
  }

  nav a.active {
    background: var(--surface-2);
    color: var(--text);
  }

  nav .back {
    background: var(--surface-2);
    color: var(--text);
    font-weight: 700;
  }

  nav .back:disabled {
    color: var(--muted);
    opacity: 0.5;
  }

  /* Icons only on phones; tablets keep the words (Android's app does the same). */
  @media (max-width: 599px) {
    nav a span,
    nav button span {
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
    /* The player bar sits over the page's bottom edge. */
    --clear-of-player: 96px;
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
  .indent {
    margin-left: 22px;
  }
</style>
