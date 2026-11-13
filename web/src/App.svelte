<script lang="ts">
  import Visualizer from './screens/Visualizer.svelte'
  import { t } from './lib/i18n.svelte'
  import { onMount } from 'svelte'
  import { api } from './lib/api'
  import Icon from './lib/Icon.svelte'
  import JoinCard from './lib/JoinCard.svelte'
  import { goBack, navigation, param, route } from './lib/router.svelte'
  import Sheet from './lib/Sheet.svelte'
  import LanguagePicker from './lib/LanguagePicker.svelte'
  import Logo from './lib/Logo.svelte'
  import RoomPicker from './lib/RoomPicker.svelte'
  import { app, connect, hasPlaylists, isAdmin, refreshMe, refreshMode } from './lib/state.svelte'
  import Toasts from './lib/Toasts.svelte'
  import Admin from './screens/Admin.svelte'
  import { adminTabFor } from './lib/adminTabs'
  import { cycleTheme, theme } from './lib/theme.svelte'
  import BackgroundArt from './lib/BackgroundArt.svelte'
  import { appearance, loadAppearance } from './lib/appearance.svelte'
  import Account from './screens/Account.svelte'
  import AlbumView from './screens/AlbumView.svelte'
  import ArtistView from './screens/ArtistView.svelte'
  import ArtistSongs from './screens/ArtistSongs.svelte'
  import GuestJoin from './screens/GuestJoin.svelte'
  import Join from './screens/Join.svelte'
  import Login from './screens/Login.svelte'
  import NowPlaying from './screens/NowPlaying.svelte'
  import Playlists from './screens/Playlists.svelte'
  import Favorites from './screens/Favorites.svelte'
  import PlaylistView from './screens/PlaylistView.svelte'
  import Queue from './screens/Queue.svelte'
  import Search from './screens/Search.svelte'
  import NotSetUp from './lib/NotSetUp.svelte'
  import Setup from './screens/Setup.svelte'
  import Signup from './screens/Signup.svelte'
  import Genre from './screens/Genre.svelte'
  import Similar from './screens/Similar.svelte'
  import Tv from './screens/Tv.svelte'
  import LyricsPage from './screens/Lyrics.svelte'
  import NodeScreen from './screens/NodeScreen.svelte'
  import SetupLibrary from './screens/SetupLibrary.svelte'

  /** So what sticks to the top of a page can stick just below this. */
  let navHeight = $state(0)

  const onSetupPage = $state(location.pathname === '/setup')
  let notSetUp = $state<Awaited<ReturnType<typeof api.bootstrapStatus>> | null>(null)
  // Invite links open /signup; the sign-in screen can switch to it too.
  let signingUp = $state(location.pathname === '/signup')
  // The join page is for guests who haven't signed in yet; the TV page is
  // public and read only.
  const onJoinPage = location.pathname === '/join'
  const onTvPage = location.pathname === '/tv'
  const onLyricsPage = location.pathname === '/lyrics'
  const onNodePage = location.pathname === '/node'
  const onVisualizerPage = location.pathname === '/visualizer'
  /** Pages that draw their own backdrop (the TV, lyrics, node and visualizer pages) skip the house background. */
  const ownBackdrop = onTvPage || onLyricsPage || onNodePage || onVisualizerPage
  if (!ownBackdrop) void loadAppearance()
  let showJoin = $state(false)
  let showMenu = $state(false)
  /** Phones keep Playlists, your account, Admin and the theme in a menu. */
  const hasMenu = $derived(!!app.me)

  onMount(async () => {
    if (onJoinPage || onTvPage || onLyricsPage || onNodePage || onVisualizerPage) return
    const me = await refreshMe()
    if (me) connect()
    else await refreshMode()
    // A box nobody has set up: there's no account to sign in with yet.
    if (!me && !onSetupPage) {
      const status = await api.bootstrapStatus().catch(() => null)
      if (status?.needed) notSetUp = status
    }
  })

  // Signing out (or being signed out) re-checks how to get back in.
  $effect(() => {
    if (app.ready && !app.me && !onJoinPage && !onTvPage && !onLyricsPage && !onNodePage && !onVisualizerPage) refreshMode()
  })

  const genreName = $derived.by(() => {
    const m = route.path.match(/^\/genre\/(.+)$/)
    return m ? decodeURIComponent(m[1]) : null
  })
  const albumId = $derived(param('/album'))
  const artistId = $derived(param('/artist'))
  const artistSongsId = $derived.by(() => {
    const m = route.path.match(/^\/artist\/(\d+)\/songs$/)
    return m ? Number(m[1]) : null
  })
  const playlistId = $derived(param('/playlist'))
  const similarId = $derived(param('/similar'))
  const adminTab = $derived(adminTabFor(route.path))
  /** Wide screens have the side menu; the header only holds the back button. */
  let wide = $state(matchMedia('(min-width: 900px)').matches)
  $effect(() => {
    const query = matchMedia('(min-width: 900px)')
    const update = () => (wide = query.matches)
    query.addEventListener('change', update)
    return () => query.removeEventListener('change', update)
  })
  const themeLabel = $derived({ auto: 'Auto', light: 'Light', dark: 'Dark' }[theme.mode])
  const tab = $derived(
    route.path.startsWith('/search') || albumId || artistId || artistSongsId || similarId || genreName
      ? 'search'
      : route.path.startsWith('/queue')
        ? 'queue'
        : route.path === '/favorites'
          ? 'favorites'
          : route.path.startsWith('/playlist')
            ? 'playlists'
          : route.path.startsWith('/admin')
            ? 'admin'
            : 'now',
  )
  /** Not on the Playing screen or in Admin, where there's nowhere to go back to. */
  const showBack = $derived(navigation.canGoBack && tab !== 'admin' && route.path !== '/')
</script>

{#if !ownBackdrop}
  <BackgroundArt name={appearance.background} strength={appearance.strength} fixed />
{/if}

{#if onTvPage}
  <Tv />
{:else if onLyricsPage}
  <LyricsPage />
{:else if onNodePage}
  <NodeScreen />
{:else if onVisualizerPage}
  <Visualizer />
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
    {:else if notSetUp}
      <NotSetUp status={notSetUp} />
    {:else if signingUp}
      <Signup onback={() => (signingUp = false)} />
    {:else if app.mode?.guest_mode && !app.hostLogin}
      <GuestJoin />
    {:else}
      <Login />
      {#if app.mode && app.mode.signup !== 'closed'}
        <button class="back-to-guest" onclick={() => (signingUp = true)}>New here? Create an account</button>
      {/if}
      {#if app.mode?.guest_mode}
        <button class="back-to-guest" onclick={() => (app.hostLogin = false)}>Not the host? Join with a name</button>
      {/if}
    {/if}
  </main>
{:else}
  <!-- The frame: on phones a header (logo, room) with tabs along the bottom;
       on wide screens a side menu with the same tabs and the rest below. -->
  <header class="topbar" bind:offsetHeight={navHeight}>
    <div class="topbar-row">
      {#if showBack}
        <button class="back" onclick={goBack} aria-label={t('nav.back')} title={t('nav.back')}>
          <Icon name="back" size={24} />
        </button>
      {/if}
      <a class="brand" href="#/" aria-label="DJINN">
        <Logo size={38} />
        <span class="brand-text">
          <strong>DJINN</strong>
          <small>{t('brand.tagline')}</small>
        </span>
      </a>
      <span class="spacer"></span>
      <button class="icon-only top-join" onclick={() => (showJoin = true)} aria-label={t('nav.join')} title={t('nav.join')}>
        <Icon name="qr" />
      </button>
      {#if hasMenu}
        <button class="icon-only top-menu" onclick={() => (showMenu = true)} aria-label={t('nav.menu')} title={t('nav.menu')}>
          <Icon name="menu" size={26} />
        </button>
      {/if}
    </div>
  </header>

  <nav class="side" class:rail={tab === 'admin'} aria-label="Main navigation">
    <a class="brand side-brand" href="#/" aria-label="DJINN">
      <Logo size={48} />
      <span class="brand-text">
        <strong>DJINN</strong>
        <small>{t('brand.tagline')}</small>
      </span>
    </a>
    <div class="tabs">
      <a href="#/" class:active={tab === 'now'}><Icon name="playing" /><span>{t('nav.playing')}</span></a>
      <a href="#/search" class:active={tab === 'search'}><Icon name="library" /><span>{t('nav.library')}</span></a>
      {#if hasPlaylists()}
        <a href="#/favorites" class:active={tab === 'favorites'}><Icon name="heart-outline" /><span>{t('nav.favorites')}</span></a>
      {/if}
      <a href="#/queue" class:active={tab === 'queue'}>
        <Icon name="queue-play" />
        <span>{t('nav.queue')}{app.queue.length ? ` (${app.queue.length})` : ''}</span>
      </a>
    </div>
    <div class="extras">
      {#if hasPlaylists()}
        <a href="#/playlists" class:active={tab === 'playlists'}><Icon name="playlist" /><span>{t('nav.playlists')}</span></a>
      {/if}
      {#if app.me.role !== 'guest'}
        <a href="#/account" class:active={route.path === '/account'}><Icon name="person" /><span>{t('playing.your_account')}</span></a>
      {/if}
      {#if isAdmin()}
        <a href="#/admin" class:active={tab === 'admin'}><Icon name="settings" /><span>Admin</span></a>
      {/if}
      <button class="theme-toggle" onclick={cycleTheme} aria-label="Theme: {themeLabel}" title="Theme: {themeLabel}">
        <Icon name="theme" /><span>{themeLabel}</span>
      </button>
      {#if tab !== 'admin'}<div class="side-room"><RoomPicker up /></div>{/if}
      <button class="tune-in" onclick={() => (showJoin = true)}>
        <Icon name="qr" size={26} />
        <span>
          <strong>{t('nav.join')}</strong>
          <small>{t('join.help_friend')}</small>
        </span>
      </button>
    </div>
  </nav>
  {#if !app.connected}
    <div class="offline">{t('app.reconnecting')}</div>
  {/if}
  <main
    class="app-main"
    class:playing={route.path === '/'}
    class:wide={tab === 'admin'}
    class:library={tab === 'search' || tab === 'favorites'}
    class:has-back={showBack}
    style:--nav-height="{wide ? 0 : navHeight}px"
  >
    {#if similarId}
      <Similar id={similarId} />
    {:else if genreName}
      <Genre name={genreName} />
    {:else if albumId}
      <AlbumView id={albumId} />
    {:else if artistId}
      <ArtistView id={artistId} />
    {:else if artistSongsId}
      <ArtistSongs id={artistSongsId} />
    {:else if playlistId && hasPlaylists()}
      <PlaylistView id={playlistId} />
    {:else if route.path === '/favorites' && hasPlaylists()}
      <Favorites />
    {:else if tab === 'search'}
      <Search />
    {:else if tab === 'queue'}
      <Queue />
    {:else if tab === 'playlists' && hasPlaylists()}
      <Playlists />
    {:else if tab === 'admin' && isAdmin()}
      <Admin tab={adminTab} />
    {:else if route.path === '/account' && app.me.role !== 'guest'}
      <Account />
    {:else}
      <NowPlaying />
    {/if}
  </main>

  {#if showMenu}
    <Sheet title={t('nav.menu')} onclose={() => (showMenu = false)}>
      {#if tab !== 'admin'}<div class="menu-room"><RoomPicker inline /></div>{/if}
      <div class="menu-list" onclick={() => (showMenu = false)} role="presentation">
        {#if hasPlaylists()}
          <a href="#/playlists"><Icon name="playlist" /><span>{t('nav.playlists')}</span></a>
        {/if}
        {#if app.me.role !== 'guest'}
          <a href="#/account"><Icon name="person" /><span>{t('playing.your_account')}</span></a>
        {/if}
        {#if isAdmin()}
          <a href="#/admin"><Icon name="settings" /><span>Admin</span></a>
        {/if}
        <button class="theme-toggle sheet-theme" onclick={cycleTheme}><Icon name="theme" /><span>Theme: {themeLabel}</span></button>
      </div>
    </Sheet>
  {/if}

  {#if showJoin}
    <Sheet title={t('join.help_friend')} onclose={() => (showJoin = false)}>
      <JoinCard />
      {#if app.me?.role === 'guest'}
        <div class="join-language"><LanguagePicker /></div>
      {/if}
    </Sheet>
  {/if}
{/if}

<Toasts />

<style>
  .join-language {
    display: flex;
    justify-content: center;
    margin-top: 16px;
  }

  main {
    max-width: 640px;
    margin: 0 auto;
    padding: calc(80px + env(safe-area-inset-top)) 16px 32px;
  }

  main.app-main {
    --main-max: 640px;
    max-width: var(--main-max);
    padding: calc(var(--nav-height, 64px) + 12px) 16px calc(var(--tabs-height) + 28px + env(safe-area-inset-bottom));
  }

  main.narrow {
    max-width: 420px;
    padding-top: 10vh;
  }

  main.app-main.playing {
    --main-max: 1300px;
  }

  /* Admin's side menu needs room next to the page. */
  main.app-main.wide {
    --main-max: 1140px;
  }

  @media (min-width: 900px) {
    main.app-main.library {
      --main-max: 1100px;
    }
  }

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

  /* Header (phones) and the slim bar above the page (wide screens). */
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
    /* Its own layer, so a phone's address bar sliding away doesn't make it jitter. */
    transform: translateZ(0);
    will-change: transform;
  }

  .topbar-row {
    display: flex;
    align-items: center;
    gap: 6px;
    max-width: 1300px;
    margin: 0 auto;
  }

  .spacer {
    flex: 1;
  }

  .menu-room {
    margin-bottom: 10px;
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

  .top-menu,
  .top-join {
    color: var(--accent);
  }

  /* Tabs: along the bottom on phones. */
  nav.side {
    position: fixed;
    left: 0;
    right: 0;
    bottom: 0;
    z-index: 10;
    padding: 0 6px env(safe-area-inset-bottom);
    background: color-mix(in srgb, var(--bg) 88%, transparent);
    backdrop-filter: blur(14px);
    border-top: 1px solid var(--edge);
    box-shadow: 0 0 18px rgb(180 140 255 / 0.18);
    transform: translateZ(0);
    will-change: transform;
  }

  .side-brand,
  .side-room,
  .extras {
    display: none;
  }

  .theme-toggle {
    justify-content: flex-start;
    gap: 12px;
    background: transparent;
  }

  .sheet-theme {
    min-height: 48px;
    padding: 12px 14px;
    border: 1px solid var(--edge);
    border-radius: 12px;
    background: var(--surface-2);
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

  .tabs a.active :global(svg),
  .extras a.active :global(svg) {
    filter: drop-shadow(0 0 5px rgb(180 140 255 / 0.6));
  }

  .menu-list {
    display: grid;
    gap: 6px;
  }

  .menu-list a {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 12px 14px;
    border: 1px solid var(--edge);
    border-radius: 12px;
    background: var(--surface-2);
    color: var(--text);
    text-decoration: none;
  }

  /* Wide screens: the concept's side menu. */
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

    .side-room {
      display: block;
    }

    .extras .theme-toggle {
      display: flex;
      align-items: center;
      padding: 10px 14px;
      border: 1px solid transparent;
      border-radius: 12px;
      color: var(--muted);
      font-size: 1rem;
    }

    .tabs a,
    .extras a {
      max-width: none;
      flex: none;
      flex-direction: row;
      justify-content: flex-start;
      gap: 12px;
      padding: 10px 14px;
      border: 1px solid transparent;
      border-radius: 12px;
      color: var(--text);
      font-size: 1rem;
      text-decoration: none;
    }

    .extras a {
      display: flex;
      align-items: center;
      color: var(--muted);
    }

    .tabs a :global(svg),
    .extras a :global(svg) {
      color: var(--accent);
    }

    .tabs a.active,
    .extras a.active {
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

    .tune-in {
      justify-content: flex-start;
      gap: 12px;
      padding: 10px 14px;
      border: 1px solid transparent;
      border-radius: 12px;
      background: transparent;
      color: var(--accent);
      text-align: left;
    }

    .tune-in:hover {
      border-color: var(--edge);
    }

    .tune-in span {
      display: flex;
      flex-direction: column;
    }

    .tune-in strong {
      color: var(--text);
      font-weight: 400;
    }

    .tune-in small {
      color: var(--muted);
      font-size: 0.75rem;
    }

    .topbar {
      left: var(--side-width);
      background: transparent;
      backdrop-filter: none;
      border-bottom: 0;
      box-shadow: none;
      pointer-events: none;
    }

    .topbar-row > :global(*) {
      pointer-events: auto;
    }

    .topbar .brand,
    .top-join,
    .top-menu {
      display: none !important;
    }

    /* On Admin pages the menu shrinks to icons, leaving room for Admin's own. */
    :global(:root:has(nav.rail)) {
      --side-width: 76px;
    }

    nav.rail {
      padding: 20px 10px;
    }

    nav.rail .brand-text {
      display: none;
    }

    /* Out of sight, but still each link's name for screen readers. */
    nav.rail .side-room {
      display: none;
    }

    nav.rail .tabs span,
    nav.rail .extras span {
      position: absolute;
      width: 1px;
      height: 1px;
      overflow: hidden;
      clip-path: inset(50%);
      white-space: nowrap;
    }

    nav.rail .side-brand {
      justify-content: center;
      padding: 0 0 14px;
    }

    nav.rail .tabs a,
    nav.rail .extras a,
    nav.rail .extras .theme-toggle,
    nav.rail .tune-in {
      justify-content: center;
      padding: 10px 0;
    }

    /* The back button sits in a gutter beside the page, so the page starts at the top. */
    main.app-main {
      --gutter: 0px;
      max-width: calc(var(--main-max) + var(--side-width) + 40px + var(--gutter));
      padding-left: calc(var(--side-width) + 24px + var(--gutter));
      padding-right: 24px;
    }

    main.app-main.has-back {
      --gutter: 52px;
    }
  }

  .back-to-guest {
    display: block;
    margin: 8px auto 0;
    background: transparent;
    color: var(--muted);
    text-decoration: underline;
  }

  .offline {
    position: fixed;
    top: calc(var(--offline-top, 72px) + env(safe-area-inset-top));
    left: 0;
    right: 0;
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
