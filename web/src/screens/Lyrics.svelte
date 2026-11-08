<script lang="ts">
  // Full-screen lyrics for a TV or spare screen: no sign-in, read only. The
  // line being sung is lit and follows the music. The admin can turn it off.
  import { onMount } from 'svelte'
  import { followNodeScreen } from '../lib/screenFollow'
  import LyricsView from '../lib/LyricsView.svelte'
  import { reloadIfUpdated } from '../lib/state.svelte'
  import type { Lyrics, NowPlaying, PlayerState, Track } from '../lib/types'

  /** Which room this screen shows (Main when missing). */
  const room = new URLSearchParams(location.search).get('room')
  const roomPart = room ? `room=${encodeURIComponent(room)}` : ''
  let nowPlaying = $state<NowPlaying>({ current: null, position_ms: 0, up_next: null })
  let player = $state<PlayerState | null>(null)
  let lyrics = $state<Lyrics | null | undefined>(undefined)
  let lyricsFor = $state<number | null>(null)
  let error = $state<string | null>(null)
  let idle = $state(false)
  /** How much later (+) or earlier (-) the words show, for speakers that run
   * behind; kept on this screen. */
  let shift = $state(readShift())

  let basePosition = 0
  let baseAt = 0
  function setPosition(ms: number) {
    basePosition = ms
    baseAt = performance.now()
  }
  const position = () =>
    (player?.state === 'playing' ? basePosition + (performance.now() - baseAt) : basePosition) - shift

  function readShift(): number {
    try {
      return Number(localStorage.getItem('djinn-lyrics-shift')) || 0
    } catch {
      return 0
    }
  }
  function nudge(ms: number) {
    shift = Math.max(-5000, Math.min(5000, shift + ms))
    try {
      localStorage.setItem('djinn-lyrics-shift', String(shift))
    } catch {
      // Not remembered.
    }
  }

  const current = $derived(nowPlaying.current?.track ?? null)
  const art = (hash: string | null, size: number) => (hash ? `/api/lyrics-page/art/${hash}?size=${size}` : null)
  const artist = (t: Track) => t.credits.map((c) => c.name).join(', ') || t.artist || ''

  let events: EventSource | null = null

  async function load() {
    try {
      const res = await fetch(roomPart ? `/api/lyrics-page?${roomPart}` : '/api/lyrics-page')
      if (!res.ok) {
        error = (await res.json().catch(() => null))?.error ?? 'The lyrics page is not available.'
        setTimeout(load, 10_000)
        return
      }
      const data = await res.json()
      nowPlaying = { current: data.current, position_ms: data.position_ms, up_next: data.up_next, later: [] }
      setPosition(data.position_ms)
      player = data.player
      lyrics = data.lyrics
      lyricsFor = data.current?.track.id ?? null
      error = null
      listen()
    } catch {
      error = 'Cannot reach the jukebox. Retrying...'
      setTimeout(load, 5_000)
    }
  }

  // A new song: fetch its words.
  $effect(() => {
    const id = current?.id ?? null
    if (id === lyricsFor) return
    lyricsFor = id
    lyrics = id == null ? null : undefined
    if (id == null) return
    fetch(`/api/lyrics-page?track=${id}${roomPart ? `&${roomPart}` : ''}`)
      .then((res) => (res.ok ? res.json() : null))
      .then((found) => {
        if (lyricsFor === id) lyrics = found
      })
      .catch(() => {
        if (lyricsFor === id) lyrics = null
      })
  })

  function listen() {
    events?.close()
    events = new EventSource(roomPart ? `/api/lyrics-page/events?${roomPart}` : '/api/lyrics-page/events')
    const on = <T,>(name: string, handle: (data: T) => void) =>
      events?.addEventListener(name, (e) => handle(JSON.parse((e as MessageEvent).data)))
    on<{ web_build: string | null }>('hello', (data) => reloadIfUpdated(data.web_build))
    on<NowPlaying>('now_playing', (data) => {
      nowPlaying = data
      setPosition(data.position_ms)
    })
    on<{ position_ms: number }>('position', (data) => setPosition(data.position_ms))
    on<PlayerState>('player_state', (data) => {
      setPosition(position() + shift)
      player = data
    })
    events.onerror = () => {
      if (events?.readyState === EventSource.CLOSED) {
        events = null
        setTimeout(load, 3_000)
      }
    }
  }

  let wakeLock: { release: () => Promise<void> } | null = null
  async function keepAwake() {
    try {
      const nav = navigator as Navigator & { wakeLock?: { request: (t: 'screen') => Promise<typeof wakeLock> } }
      if (document.visibilityState === 'visible' && nav.wakeLock) wakeLock = await nav.wakeLock.request('screen')
    } catch {
      // The screen's own settings decide.
    }
  }

  let idleTimer: ReturnType<typeof setTimeout>
  function wake() {
    idle = false
    clearTimeout(idleTimer)
    idleTimer = setTimeout(() => (idle = true), 3000)
  }

  function toggleFullscreen() {
    if (document.fullscreenElement) document.exitFullscreen()
    else document.documentElement.requestFullscreen?.().catch(() => {})
    keepAwake()
  }

  onMount(() => {
    document.title = 'Lyrics · DJINN'
    load()
    keepAwake()
    wake()
    const onVisible = () => keepAwake()
    document.addEventListener('visibilitychange', onVisible)
    const stopFollowing = followNodeScreen()
    return () => {
      stopFollowing()
      events?.close()
      wakeLock?.release()
      document.removeEventListener('visibilitychange', onVisible)
    }
  })
</script>

<svelte:window onmousemove={wake} onkeydown={wake} ontouchstart={wake} />

<div class="page" class:idle>
  {#if current?.art_hash}
    <div class="backdrop" style:background-image="url({art(current.art_hash, 200)})"></div>
  {/if}
  {#if error}
    <p class="message">{error}</p>
  {:else if !current}
    <p class="message">Nothing playing right now.</p>
  {:else}
    <header>
      {#if current.art_hash}<img src={art(current.art_hash, 160)} alt="" />{/if}
      <div>
        <h1>{current.title}</h1>
        <p>{artist(current)}</p>
      </div>
    </header>
    <main>
      <LyricsView {lyrics} {position} big empty="No lyrics for this song." />
    </main>
  {/if}
  <div class="tools">
    {#if lyrics?.lines}
      <button onclick={() => nudge(-250)} title="Show words earlier">Earlier</button>
      <span>{shift === 0 ? 'On time' : `${shift > 0 ? '+' : ''}${(shift / 1000).toFixed(2)} s`}</span>
      <button onclick={() => nudge(250)} title="Show words later">Later</button>
    {/if}
    <button onclick={toggleFullscreen}>Full screen</button>
  </div>
</div>

<style>
  .page {
    position: fixed;
    inset: 0;
    display: flex;
    flex-direction: column;
    padding: max(24px, env(safe-area-inset-top)) max(24px, env(safe-area-inset-right))
      max(24px, env(safe-area-inset-bottom)) max(24px, env(safe-area-inset-left));
    background: var(--bg);
    color: var(--text);
    overflow: hidden;
  }

  .page.idle {
    cursor: none;
  }

  .backdrop {
    position: absolute;
    inset: -40px;
    background-size: cover;
    background-position: center;
    filter: blur(40px) saturate(1.2);
    opacity: 0.25;
  }

  header,
  main,
  .tools,
  .message {
    position: relative;
  }

  header {
    display: flex;
    align-items: center;
    gap: 16px;
    flex: none;
  }

  header img {
    width: clamp(56px, 8vw, 96px);
    height: clamp(56px, 8vw, 96px);
    border-radius: 10px;
    object-fit: cover;
  }

  header h1 {
    margin: 0;
    font-size: clamp(1.2rem, 2.6vw, 2.2rem);
  }

  header p {
    margin: 2px 0 0;
    color: var(--muted);
    font-size: clamp(0.95rem, 1.8vw, 1.4rem);
  }

  main {
    flex: 1;
    min-height: 0;
    margin-top: 12px;
  }

  .message {
    margin: auto;
    color: var(--muted);
    font-size: 1.5rem;
    text-align: center;
  }

  .tools {
    position: absolute;
    right: max(16px, env(safe-area-inset-right));
    bottom: max(16px, env(safe-area-inset-bottom));
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 0.875rem;
    color: var(--muted);
    transition: opacity 0.3s;
  }

  .idle .tools {
    opacity: 0;
    pointer-events: none;
  }

  .tools button {
    min-height: 36px;
    padding: 6px 12px;
    background: var(--surface-2);
    color: var(--text);
  }
</style>
