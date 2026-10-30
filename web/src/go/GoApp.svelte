<script lang="ts">
  // DJINN GO: your own listening, apart from the party. Sign in, find a
  // song, and it plays here; the song file comes as it is and this device
  // plays it. Stage 3 adds your own queue, likes, playlists and history.
  import { onMount } from 'svelte'
  import AccountPanels from '../lib/AccountPanels.svelte'
  import { api, ApiError } from '../lib/api'
  import Art from '../lib/Art.svelte'
  import { duration } from '../lib/format'
  import { app } from '../lib/state.svelte'
  import Toasts from '../lib/Toasts.svelte'
  import TwoStepPanel from '../lib/TwoStepPanel.svelte'
  import type { Track } from '../lib/types'
  import Login from '../screens/Login.svelte'
  import Signup from '../screens/Signup.svelte'

  let ready = $state(false)
  let signingUp = $state(location.pathname === '/signup')
  let view = $state<'music' | 'account'>('music')
  let query = $state('')
  let tracks = $state<Track[]>([])
  let listTitle = $state('Newest songs')
  let current = $state<Track | null>(null)
  let downloadUrl = $state<string | null>(null)
  let error = $state('')
  let audio = $state<HTMLAudioElement | undefined>()
  let searchTimer: ReturnType<typeof setTimeout> | undefined

  onMount(async () => {
    await refresh()
    ready = true
  })

  async function refresh() {
    try {
      app.me = await api.me()
    } catch (e) {
      app.me = null
      if (e instanceof ApiError && e.status !== 401) error = e.message
      app.mode = await api.mode().catch(() => null)
    }
    if (canListen()) await loadNewest()
  }

  const canListen = () =>
    !!app.me && !!app.me.permissions?.go && !(app.me.two_step_required && !app.me.two_step)

  async function signedIn() {
    error = ''
    signingUp = false
    await refresh()
  }

  async function loadNewest() {
    try {
      tracks = (await api.tracks('added', 0)).items
      listTitle = 'Newest songs'
    } catch (e) {
      error = e instanceof Error ? e.message : 'Could not load songs'
    }
  }

  function typed() {
    clearTimeout(searchTimer)
    searchTimer = setTimeout(async () => {
      const q = query.trim()
      if (!q) return loadNewest()
      try {
        tracks = (await api.search(q)).tracks
        listTitle = `Songs matching "${q}"`
      } catch (e) {
        error = e instanceof Error ? e.message : 'Search failed'
      }
    }, 250)
  }

  async function play(track: Track) {
    error = ''
    try {
      const links = await api.playLinks(track.id)
      current = track
      downloadUrl = links.download_url
      if (audio) {
        audio.src = links.url
        await audio.play()
      }
      setMediaSession(track)
    } catch (e) {
      error = e instanceof Error ? e.message : 'Could not play that song'
    }
  }

  /** Plays the next song in the list when one ends. */
  function next() {
    if (!current) return
    const at = tracks.findIndex((t) => t.id === current!.id)
    const following = tracks[at + 1]
    if (following) play(following)
  }

  function previous() {
    if (!current) return
    const at = tracks.findIndex((t) => t.id === current!.id)
    if (at > 0) play(tracks[at - 1])
  }

  function setMediaSession(track: Track) {
    if (!('mediaSession' in navigator)) return
    navigator.mediaSession.metadata = new MediaMetadata({
      title: track.title,
      artist: track.artist ?? '',
      album: track.album ?? '',
      artwork: track.art_hash ? [{ src: `/api/art/${track.art_hash}?size=512`, sizes: '512x512' }] : [],
    })
    navigator.mediaSession.setActionHandler('nexttrack', next)
    navigator.mediaSession.setActionHandler('previoustrack', previous)
  }

  /** The browser couldn't play the file: often a format it doesn't know. */
  function playbackFailed() {
    if (current) error = `This device can't play "${current.title}". Try another song.`
  }

  async function signOut() {
    audio?.pause()
    await api.logout().catch(() => {})
    app.me = null
    current = null
    app.mode = await api.mode().catch(() => null)
  }
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
  <main class="go" class:playing={current}>
    <header>
      <h1>DJINN GO</h1>
      <nav>
        <button class:active={view === 'music'} onclick={() => (view = 'music')}>Music</button>
        <button class:active={view === 'account'} onclick={() => (view = 'account')}>Account</button>
      </nav>
    </header>

    {#if view === 'account'}
      <p class="muted">Signed in as <b>{app.me.username}</b>{#if app.me.role_name} ({app.me.role_name}){/if}.</p>
      <AccountPanels />
      <button class="signout" onclick={signOut}>Sign out</button>
    {:else}
      <input
        class="search"
        type="search"
        placeholder="Search songs, artists, albums"
        bind:value={query}
        oninput={typed}
        aria-label="Search"
      />
      {#if error}<p class="error">{error}</p>{/if}
      <h2>{listTitle}</h2>
      {#each tracks as track (track.id)}
        <button class="track" class:current={current?.id === track.id} onclick={() => play(track)}>
          <Art hash={track.art_hash} size={44} />
          <span class="grow">
            <span class="title">{track.title}</span>
            <span class="muted small">{[track.artist, track.album].filter(Boolean).join(' · ')}</span>
          </span>
          <span class="muted small">{duration(track.duration_ms)}</span>
        </button>
      {:else}
        <p class="muted">No songs found.</p>
      {/each}
    {/if}
  </main>
{/if}

<div class="player" hidden={!app.me || !current}>
  {#if current}
    <div class="now">
      <Art hash={current.art_hash} size={44} />
      <div class="grow">
        <div class="title">{current.title}</div>
        <div class="muted small">{current.artist ?? ''}</div>
      </div>
      {#if downloadUrl}<a class="small" href={downloadUrl} download>Download</a>{/if}
    </div>
  {/if}
  <audio bind:this={audio} controls onended={next} onerror={playbackFailed}></audio>
</div>

<style>
  .go {
    max-width: 720px;
    margin: 0 auto;
    padding: 16px 16px 24px;
  }

  .go.playing {
    padding-bottom: 150px;
  }

  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    margin-bottom: 12px;
  }

  header h1 {
    margin: 0;
  }

  nav {
    display: flex;
    gap: 6px;
  }

  nav button {
    min-height: 36px;
    padding: 0 14px;
    border-radius: 999px;
  }

  nav button.active {
    background: var(--accent);
    color: var(--accent-text);
  }

  .search {
    width: 100%;
  }

  h2 {
    margin: 18px 0 6px;
    font-size: 1rem;
  }

  .track {
    display: flex;
    align-items: center;
    gap: 12px;
    width: 100%;
    padding: 6px 4px;
    border-radius: 10px;
    background: transparent;
    color: inherit;
    text-align: left;
  }

  .track.current .title {
    color: var(--accent);
  }

  .grow {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-width: 0;
  }

  .title {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .player {
    position: fixed;
    left: 0;
    right: 0;
    bottom: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 10px 16px calc(10px + env(safe-area-inset-bottom));
    border-top: 1px solid var(--line);
    background: var(--surface);
  }

  .player[hidden] {
    display: none;
  }

  .now {
    display: flex;
    align-items: center;
    gap: 12px;
    max-width: 720px;
    width: 100%;
    margin: 0 auto;
  }

  audio {
    width: 100%;
    max-width: 720px;
    margin: 0 auto;
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
