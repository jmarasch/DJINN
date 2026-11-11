<script lang="ts">
  // Full-screen lyrics for a TV or spare screen: no sign-in, read only. The
  // line being sung is lit and follows the music; only timed words show. The
  // song is in the top left, a small QR code to join in the lower left and
  // what's coming up down the right. On a node's screen this is also the
  // visualizer's page: the node draws the visual under it, and everything
  // here fades back so the visual stands out. The admin can turn it off.
  import { onMount } from 'svelte'
  import { followNodeScreen } from '../lib/screenFollow'
  import {
    announce,
    choose,
    onNodeScreen,
    order,
    pageUrl,
    toggleFullscreen,
    type ScreenPage,
  } from '../lib/screenOrder.svelte'
  import LyricsView from '../lib/LyricsView.svelte'
  import { reloadIfUpdated } from '../lib/state.svelte'
  import type { Entry, Lyrics, NowPlaying, PlayerState, QueueItem, Track } from '../lib/types'

  /** Songs in the Coming up list. */
  const COMING_UP = 5
  /** Which room this screen shows (Main when missing). */
  const room = new URLSearchParams(location.search).get('room')
  const roomPart = room ? `room=${encodeURIComponent(room)}` : ''
  let nowPlaying = $state<NowPlaying>({ current: null, position_ms: 0, up_next: null })
  let player = $state<PlayerState | null>(null)
  let queue = $state<QueueItem[]>([])
  let joinUrl = $state('')
  let lyrics = $state<Lyrics | null | undefined>(undefined)
  let lyricsFor = $state<number | null>(null)
  let error = $state<string | null>(null)
  let idle = $state(false)
  /** The feed has answered, and a moment has passed for the node to say
   * whether it has sound, so the screen doesn't jump about while starting. */
  let settled = $state(false)
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

  /** Only timed words: plain ones can't follow the song. */
  const timed = $derived(lyrics === undefined ? undefined : lyrics?.lines?.length ? lyrics : null)
  const hasTimed = $derived(lyrics === undefined ? current?.lyrics === 2 : !!timed)

  /** What this screen shows: always lyrics, unless it's a node's screen with
   * an order to follow. */
  const page = $derived<ScreenPage>(
    order.length && settled ? choose({ playing: !!current, timed: hasTimed }) : (order[0] ?? 'lyrics'),
  )
  const visual = $derived(page === 'visual')

  $effect(() => {
    if (!settled) return
    if (page === 'tv') location.replace(pageUrl('tv'))
    else announce(page, current?.id ?? null)
  })

  // The node draws under this page: see-through while the visual shows.
  $effect(() => {
    const clear = visual ? 'transparent' : ''
    document.documentElement.style.background = clear
    document.body.style.background = clear
  })

  /** Requests first, then the auto mix's songs. */
  const comingUp = $derived.by(() => {
    const songs: { key: string; track: Track; who: string | null }[] = queue.map((item) => ({
      key: `q${item.id}`,
      track: item.track,
      who: item.username,
    }))
    const auto: Entry[] = [
      ...(nowPlaying.up_next && nowPlaying.up_next.source !== 'request' && nowPlaying.up_next.track.id !== current?.id
        ? [nowPlaying.up_next]
        : []),
      ...(nowPlaying.later ?? []),
    ]
    auto.forEach((entry, i) => songs.push({ key: `a${entry.track.id}-${i}`, track: entry.track, who: null }))
    return songs.slice(0, COMING_UP)
  })

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
      nowPlaying = { current: data.current, position_ms: data.position_ms, up_next: data.up_next, later: data.later ?? [] }
      setPosition(data.position_ms)
      player = data.player
      lyrics = data.lyrics
      lyricsFor = data.current?.track.id ?? null
      queue = data.queue ?? []
      joinUrl = data.join_url ?? ''
      error = null
      listen()
      setTimeout(() => (settled = true), 2500)
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
    on<QueueItem[]>('queue_changed', (data) => (queue = data))
    on<{ url: string }>('address', (data) => (joinUrl = data.url))
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

  function goFullscreen() {
    toggleFullscreen()
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

<div class="page" class:idle class:visual>
  {#if current?.art_hash && !visual}
    <div class="backdrop" style:background-image="url({art(current.art_hash, 200)})"></div>
  {/if}
  {#if error}
    <p class="message">{error}</p>
  {:else}
    <header class="faded">
      {#if current}
        {#if current.art_hash}<img src={art(current.art_hash, 160)} alt="" />{/if}
        <div>
          <h1>{current.title}</h1>
          <p>{artist(current)}</p>
          {#if current.album}<p class="album">{current.album}</p>{/if}
        </div>
      {/if}
    </header>
    <main>
      {#if !current}
        <p class="message">Nothing playing right now.</p>
      {:else if !visual}
        <LyricsView lyrics={timed} {position} big empty="No timed lyrics for this song." />
      {/if}
    </main>
    {#if comingUp.length}
      <aside class="faded">
        <h2>Coming up</h2>
        <ol>
          {#each comingUp as song (song.key)}
            <li>
              {#if song.track.art_hash}
                <img src={art(song.track.art_hash, 96)} alt="" />
              {:else}
                <div class="no-art">♪</div>
              {/if}
              <div class="text">
                <div class="title">{song.track.title}</div>
                <div class="sub">{artist(song.track)}</div>
                {#if song.who}<div class="sub who">{song.who}</div>{/if}
              </div>
            </li>
          {/each}
        </ol>
      </aside>
    {/if}
    {#if joinUrl}
      <img class="qr faded" src="/api/join/qr.svg?v={encodeURIComponent(joinUrl)}" alt="QR code to join" />
    {/if}
  {/if}
  <div class="tools">
    {#if timed?.lines && !visual}
      <button onclick={() => nudge(-250)} title="Show words earlier">Earlier</button>
      <span>{shift === 0 ? 'On time' : `${shift > 0 ? '+' : ''}${(shift / 1000).toFixed(2)} s`}</span>
      <button onclick={() => nudge(250)} title="Show words later">Later</button>
    {/if}
    {#if !onNodeScreen}
      <!-- The whole window, not just a visualizer's frame. -->
      <a class="button" href="/#/" target="_top">Regular display</a>
    {/if}
    <button onclick={goFullscreen}>Full screen</button>
  </div>
</div>

<style>
  .page {
    --side: clamp(160px, 20vw, 360px);
    --edge: max(24px, 2.5vw);
    position: fixed;
    inset: 0;
    background: var(--bg);
    color: var(--text);
    overflow: hidden;
  }

  .page.visual {
    background: transparent;
    color: #fff;
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

  /* Everything but the visual steps back while it shows. */
  .visual .faded {
    opacity: 0.45;
  }

  header {
    position: absolute;
    top: var(--edge);
    left: var(--edge);
    right: calc(var(--side) + 2 * var(--edge));
    display: flex;
    align-items: center;
    gap: 16px;
    transition: opacity 0.4s;
  }

  header img {
    width: clamp(56px, 7vw, 96px);
    height: clamp(56px, 7vw, 96px);
    border-radius: 10px;
    object-fit: cover;
    flex: none;
  }

  header div {
    min-width: 0;
  }

  header h1 {
    margin: 0;
    font-size: clamp(1.1rem, 2.4vw, 2.1rem);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  header p {
    margin: 2px 0 0;
    color: var(--muted);
    font-size: clamp(0.9rem, 1.6vw, 1.3rem);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .visual header p {
    color: rgb(255 255 255 / 0.8);
  }

  header p.album {
    font-size: clamp(0.8rem, 1.3vw, 1.1rem);
  }

  main {
    position: absolute;
    top: calc(var(--edge) + clamp(56px, 7vw, 96px) + 16px);
    bottom: var(--edge);
    left: calc(var(--edge) + clamp(72px, 9vw, 140px));
    right: calc(var(--side) + 2 * var(--edge));
    display: flex;
    flex-direction: column;
  }

  main > :global(*) {
    flex: 1;
    min-height: 0;
  }

  .message {
    margin: auto;
    color: var(--muted);
    font-size: 1.5rem;
    text-align: center;
  }

  .page > .message {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
  }

  aside {
    position: absolute;
    top: var(--edge);
    right: var(--edge);
    bottom: calc(var(--edge) + 56px);
    width: var(--side);
    overflow: hidden;
    transition: opacity 0.4s;
  }

  aside h2 {
    margin: 0 0 10px;
    font-size: clamp(0.8rem, 1.1vw, 1.1rem);
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--muted);
  }

  .visual aside h2 {
    color: rgb(255 255 255 / 0.75);
  }

  ol {
    margin: 0;
    padding: 0;
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  li {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
  }

  li img,
  .no-art {
    width: clamp(32px, 3.2vw, 56px);
    height: clamp(32px, 3.2vw, 56px);
    flex: none;
    border-radius: 6px;
    object-fit: cover;
  }

  .no-art {
    display: grid;
    place-items: center;
    background: var(--surface-2);
    color: var(--muted);
  }

  .text {
    min-width: 0;
  }

  .title,
  .sub {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .title {
    font-size: clamp(0.8rem, 1.1vw, 1.1rem);
    font-weight: 600;
  }

  .sub {
    font-size: clamp(0.7rem, 0.95vw, 0.95rem);
    color: var(--muted);
  }

  .visual .sub {
    color: rgb(255 255 255 / 0.7);
  }

  .qr {
    position: absolute;
    left: var(--edge);
    bottom: var(--edge);
    width: clamp(72px, 9vw, 140px);
    height: clamp(72px, 9vw, 140px);
    padding: 6px;
    border-radius: 8px;
    background: #fff;
    transition: opacity 0.4s;
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

  .tools button,
  .tools .button {
    min-height: 36px;
    padding: 6px 12px;
    background: var(--surface-2);
    color: var(--text);
  }

  .tools .button {
    display: inline-flex;
    align-items: center;
    border-radius: 8px;
    text-decoration: none;
  }

  @media (max-width: 640px) {
    .page {
      --side: 0px;
    }

    aside {
      display: none;
    }

    main {
      left: var(--edge);
      bottom: calc(var(--edge) + 88px);
    }
  }
</style>
