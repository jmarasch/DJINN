<script lang="ts">
  import { t } from '../lib/i18n.svelte'
  // DJINN GO: your own listening, apart from the party. The song files
  // come as they are and this device plays them.
  import { onMount } from 'svelte'
  import AccountPanels from '../lib/AccountPanels.svelte'
  import EqualizerPanel from '../lib/EqualizerPanel.svelte'
  import { loadEqualizer, saveEqualizer } from './equalizer.svelte'
  import { api, ApiError, onUnauthorized } from '../lib/api'
  import Icon from '../lib/Icon.svelte'
  import Logo from '../lib/Logo.svelte'
  import PageTitle from '../lib/PageTitle.svelte'
  import BackgroundArt from '../lib/BackgroundArt.svelte'
  import { appearance, loadAppearance } from '../lib/appearance.svelte'
  import { cycleTheme, theme } from '../lib/theme.svelte'
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

  /** Wide screens have the side menu; the header only holds the back button. */
  let wide = $state(matchMedia('(min-width: 900px)').matches)
  $effect(() => {
    const query = matchMedia('(min-width: 900px)')
    const update = () => (wide = query.matches)
    query.addEventListener('change', update)
    return () => query.removeEventListener('change', update)
  })

  let ready = $state(false)
  let signingUp = $state(location.pathname === '/signup')
  let error = $state('')

  onMount(async () => {
    void loadAppearance()
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

  const themeLabel = $derived({ auto: 'Auto', light: 'Light', dark: 'Dark' }[theme.mode])
  /** Not on Home, where there's nowhere to go back to. */
  const showBack = $derived(navigation.canGoBack && route.path !== '/')
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
<BackgroundArt name={appearance.background} strength={appearance.strength} fixed />
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
  <header class="topbar" bind:offsetHeight={headerHeight}>
    <div class="topbar-row">
      {#if showBack}
        <button class="back" onclick={goBack} aria-label={t('nav.back')} title={t('nav.back')}>
          <Icon name="back" size={24} />
        </button>
      {/if}
      <a class="brand" href="#/" aria-label="DJINN GO">
        <Logo size={38} />
        <span class="brand-text">
          <strong>DJINN GO</strong>
          <small>{t('brand.tagline')}</small>
        </span>
      </a>
    </div>
  </header>
  <nav class="side" aria-label="Main navigation">
    <a class="brand side-brand" href="#/" aria-label="DJINN GO">
      <Logo size={48} />
      <span class="brand-text">
        <strong>DJINN GO</strong>
        <small>{t('brand.tagline')}</small>
      </span>
    </a>
    <div class="tabs">
      <a href="#/" class:active={section === 'home'} aria-label={t('go.home')}><Icon name="home" /><span>{t('go.home')}</span></a>
      <a href="#/search" class:active={section === 'search'} aria-label={t('nav.library')}><Icon name="library" /><span>{t('nav.library')}</span></a>
      <a href="#/library" class:active={section === 'library'} aria-label={t('go.your_music')}>
        <Icon name="heart-outline" /><span>{t('go.your_music')}</span>
      </a>
      <a href="#/account" class:active={section === 'account'} aria-label={t('go.account')}><Icon name="person" /><span>{t('go.account')}</span></a>
    </div>
    <div class="extras">
      <button class="theme-toggle" onclick={cycleTheme} aria-label="Theme: {themeLabel}" title="Theme: {themeLabel}">
        <Icon name="theme" /><span>{themeLabel}</span>
      </button>
    </div>
  </nav>
  <main
    class="go"
    class:playing={player.index >= 0 && route.path !== '/playing'}
    class:has-back={showBack}
    style:--nav-height="{wide ? 0 : headerHeight}px"
  >
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
      <PageTitle title="Your account" />
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
      <EqualizerPanel load={loadEqualizer} store={saveEqualizer} note={t('eq.yours_note')} />
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
  :global(:root) {
    --tabs-height: 64px;
    --side-width: 248px;
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
    color: var(--text);
    text-decoration: none;
  }

  .brand-text {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .brand-text strong {
    font-size: 1.375rem;
    letter-spacing: 0.12em;
    line-height: 1.05;
    text-shadow: 0 0 14px rgb(180 140 255 / 0.35);
  }

  .brand-text small {
    overflow: hidden;
    color: var(--muted);
    font-size: 0.5625rem;
    letter-spacing: 0.16em;
    text-transform: uppercase;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  /* Phones: a header with the logo, tabs along the bottom. */
  .topbar {
    position: fixed;
    top: 0;
    left: 0;
    right: 0;
    z-index: 10;
    padding: calc(8px + env(safe-area-inset-top)) 12px 8px;
    background: color-mix(in srgb, var(--bg) 86%, transparent);
    backdrop-filter: blur(14px);
    border-bottom: 1px solid var(--edge);
    box-shadow: var(--glow);
    transform: translateZ(0);
    will-change: transform;
  }

  .topbar-row {
    display: flex;
    align-items: center;
    gap: 8px;
    max-width: 1100px;
    margin: 0 auto;
  }

  .back {
    flex: none;
    width: 40px;
    min-height: 40px;
    padding: 0;
    border: 1px solid var(--edge);
    border-radius: 12px;
    background: var(--surface-2);
    color: var(--text);
  }

  nav.side {
    position: fixed;
    left: 0;
    right: 0;
    bottom: 0;
    z-index: 30;
    padding: 0 6px env(safe-area-inset-bottom);
    background: color-mix(in srgb, var(--bg) 88%, transparent);
    backdrop-filter: blur(14px);
    border-top: 1px solid var(--edge);
    box-shadow: 0 0 18px rgb(180 140 255 / 0.18);
    transform: translateZ(0);
    will-change: transform;
  }

  .side-brand,
  .extras {
    display: none;
  }

  .tabs {
    display: flex;
    justify-content: space-around;
    height: var(--tabs-height);
  }

  .tabs a {
    position: relative;
    flex: 1;
    max-width: 120px;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 3px;
    color: var(--muted);
    font-size: 0.75rem;
    text-decoration: none;
  }

  .tabs a.active {
    color: var(--accent);
    text-shadow: 0 0 10px rgb(180 140 255 / 0.5);
  }

  .tabs a.active::after {
    content: '';
    position: absolute;
    left: 22%;
    right: 22%;
    bottom: 6px;
    height: 3px;
    border-radius: 3px;
    background: var(--accent);
    box-shadow: var(--glow-strong);
  }

  .tabs a.active :global(svg) {
    filter: drop-shadow(0 0 5px rgb(180 140 255 / 0.6));
  }

  .go {
    --main-max: 760px;
    --gutter: 0px;
    max-width: var(--main-max);
    margin: 0 auto;
    padding: calc(var(--nav-height, 64px) + 12px) 16px calc(var(--tabs-height) + 28px + env(safe-area-inset-bottom));
  }

  .go.playing {
    padding-bottom: calc(var(--tabs-height) + 104px + env(safe-area-inset-bottom));
    /* The player bar sits over the page's bottom edge. */
    --clear-of-player: calc(var(--tabs-height) + 76px);
  }

  /* Wide screens: the side menu, as in the party app. */
  @media (min-width: 900px) {
    :global(:root) {
      --tabs-height: 0px;
    }

    nav.side {
      top: 0;
      right: auto;
      width: var(--side-width);
      display: flex;
      flex-direction: column;
      gap: 18px;
      padding: 20px 14px;
      overflow-y: auto;
      border-top: 0;
      border-right: 1px solid var(--edge);
      background: color-mix(in srgb, var(--bg) 92%, transparent);
    }

    .side-brand {
      display: flex;
      padding: 0 6px 16px;
      border-bottom: 1px solid var(--edge);
    }

    .side-brand small {
      white-space: normal;
      line-height: 1.4;
    }

    .tabs,
    .extras {
      display: flex;
      flex-direction: column;
      gap: 4px;
      height: auto;
    }

    .extras {
      padding-top: 14px;
      border-top: 1px solid var(--edge);
    }

    .tabs a,
    .extras .theme-toggle {
      max-width: none;
      flex: none;
      flex-direction: row;
      justify-content: flex-start;
      gap: 12px;
      padding: 10px 14px;
      border: 1px solid transparent;
      border-radius: 12px;
      background: transparent;
      color: var(--text);
      font-size: 1rem;
      text-decoration: none;
    }

    .extras .theme-toggle {
      color: var(--muted);
    }

    .tabs a :global(svg),
    .extras :global(svg) {
      color: var(--accent);
    }

    .tabs a.active {
      border-color: var(--edge-strong);
      background: linear-gradient(90deg, rgb(180 140 255 / 0.5), rgb(180 140 255 / 0.28));
      box-shadow: var(--glow-strong);
      color: var(--text);
      text-shadow: none;
    }

    .tabs a.active::after {
      display: none;
    }

    .tabs a.active :global(svg) {
      color: var(--text);
    }

    .topbar {
      left: var(--side-width);
      background: transparent;
      backdrop-filter: none;
      border-bottom: 0;
      box-shadow: none;
      pointer-events: none;
    }

    .topbar .back {
      pointer-events: auto;
    }

    .topbar .brand {
      display: none;
    }

    /* The back button sits in a gutter beside the page, so the page starts at the top. */
    .go {
      --main-max: 1000px;
      max-width: calc(var(--main-max) + var(--side-width) + 40px + var(--gutter));
      padding-left: calc(var(--side-width) + 24px + var(--gutter));
      padding-right: 24px;
      padding-top: 12px;
    }

    .go.has-back {
      --gutter: 52px;
    }
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
