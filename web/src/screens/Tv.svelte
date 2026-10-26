<script lang="ts">
  // Full-screen now playing for a TV or spare tablet: no sign-in, read only.
  import { onMount } from 'svelte'
  import { duration } from '../lib/format'
  import type { NowPlaying, PlayerState, QueueItem, Track } from '../lib/types'

  const UP_NEXT = 6

  let nowPlaying = $state<NowPlaying>({ current: null, position_ms: 0, up_next: null })
  let player = $state<PlayerState | null>(null)
  let queue = $state<QueueItem[]>([])
  let joinUrl = $state('')
  let guestMode = $state(false)
  let error = $state<string | null>(null)
  let position = $state(0)
  let idleCursor = $state(false)

  // Position from the server, extrapolated while playing.
  let basePosition = 0
  let baseAt = 0
  function setPosition(ms: number) {
    basePosition = ms
    baseAt = performance.now()
    position = ms
  }

  const art = (hash: string | null, size?: number) =>
    hash ? `/api/tv/art/${hash}${size ? `?size=${size}` : ''}` : null
  const artist = (t: Track) => {
    if (!t.credits.length) return t.artist ?? ''
    const main = t.credits.filter((c) => !c.featured).map((c) => c.name)
    const feat = t.credits.filter((c) => c.featured).map((c) => c.name)
    const list = (names: string[]) =>
      names.length < 2 ? names.join('') : `${names.slice(0, -1).join(', ')} & ${names[names.length - 1]}`
    return feat.length ? `${list(main)} feat. ${list(feat)}` : list(main)
  }

  let events: EventSource | null = null

  async function load() {
    try {
      const res = await fetch('/api/tv')
      if (!res.ok) {
        error = (await res.json().catch(() => null))?.error ?? 'The TV page is not available.'
        setTimeout(load, 10_000)
        return
      }
      const data = await res.json()
      nowPlaying = { current: data.current, position_ms: data.position_ms, up_next: data.up_next }
      setPosition(data.position_ms)
      player = data.player
      queue = data.queue
      joinUrl = data.join_url
      guestMode = data.guest_mode
      error = null
      listen()
    } catch {
      error = 'Cannot reach the jukebox. Retrying...'
      setTimeout(load, 5_000)
    }
  }

  function listen() {
    events?.close()
    events = new EventSource('/api/tv/events')
    const on = <T,>(name: string, handle: (data: T) => void) =>
      events?.addEventListener(name, (e) => handle(JSON.parse((e as MessageEvent).data)))
    on<NowPlaying>('now_playing', (data) => {
      nowPlaying = data
      setPosition(data.position_ms)
    })
    on<{ position_ms: number }>('position', (data) => setPosition(data.position_ms))
    on<QueueItem[]>('queue_changed', (data) => (queue = data))
    on<PlayerState>('player_state', (data) => {
      setPosition(position)
      player = data
    })
    events.onerror = () => {
      // Closed for good (turned off, or the server restarted): start over.
      if (events?.readyState === EventSource.CLOSED) {
        events = null
        setTimeout(load, 3_000)
      }
    }
  }

  // Keep the screen awake while the page is visible.
  let wakeLock: { release: () => Promise<void> } | null = null
  async function keepAwake() {
    try {
      const nav = navigator as Navigator & { wakeLock?: { request: (t: 'screen') => Promise<typeof wakeLock> } }
      if (document.visibilityState === 'visible' && nav.wakeLock) wakeLock = await nav.wakeLock.request('screen')
    } catch {
      // Not allowed (no user gesture yet, or unsupported); the TV's own
      // settings decide.
    }
  }

  let cursorTimer: ReturnType<typeof setTimeout>
  function showCursor() {
    idleCursor = false
    clearTimeout(cursorTimer)
    cursorTimer = setTimeout(() => (idleCursor = true), 3000)
  }

  function toggleFullscreen() {
    if (document.fullscreenElement) document.exitFullscreen()
    else document.documentElement.requestFullscreen?.().catch(() => {})
    keepAwake()
  }

  onMount(() => {
    load()
    keepAwake()
    showCursor()
    const tick = setInterval(() => {
      if (player?.state === 'playing') position = basePosition + (performance.now() - baseAt)
    }, 250)
    const onVisible = () => keepAwake()
    document.addEventListener('visibilitychange', onVisible)
    return () => {
      clearInterval(tick)
      events?.close()
      wakeLock?.release()
      document.removeEventListener('visibilitychange', onVisible)
    }
  })

  const current = $derived(nowPlaying.current)
  const total = $derived(current?.track.duration_ms ?? 0)
  const shown = $derived(Math.min(position, total || position))
  const upNext = $derived(queue.slice(0, UP_NEXT))
  const more = $derived(Math.max(0, queue.length - UP_NEXT))
</script>

<svelte:head><title>DJINN TV</title></svelte:head>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="tv" class:idle-cursor={idleCursor} onmousemove={showCursor}>
  {#if current?.track.art_hash}
    <div class="backdrop" style:background-image="url({art(current.track.art_hash, 512)})"></div>
  {/if}
  <div class="shade"></div>

  {#if error}
    <div class="message">
      <h1>DJINN</h1>
      <p>{error}</p>
    </div>
  {:else}
    <main class="now">
      {#if current}
        <div class="cover">
          {#if current.track.art_hash}
            <img src={art(current.track.art_hash)} alt="" />
          {:else}
            <div class="no-art">♪</div>
          {/if}
        </div>
        <div class="info">
          <div class="label">{player?.state === 'paused' ? 'Paused' : 'Now playing'}</div>
          <h1>{current.track.title}</h1>
          <p class="artist">{artist(current.track)}</p>
          {#if current.track.album}<p class="album">{current.track.album}</p>{/if}
          {#if total}
            <div class="progress" aria-hidden="true">
              <div style:width="{Math.min(100, (shown / total) * 100)}%"></div>
            </div>
            <div class="times"><span>{duration(shown)}</span><span>{duration(total)}</span></div>
          {/if}
          <p class="by">
            {#if current.requested_by}
              Requested by <strong>{current.requested_by}</strong>{#if current.backers.length}
                and {current.backers.length === 1 ? current.backers[0] : `${current.backers.length} others`}{/if}
            {:else if player?.playlist}
              From the playlist <strong>{player.playlist.name}</strong>
            {:else}
              From the party mix
            {/if}
          </p>
        </div>
      {:else}
        <div class="message">
          <h1>DJINN</h1>
          <p>Scan the code to start the music.</p>
        </div>
      {/if}
    </main>

    <aside class="side">
      <h2>Up next</h2>
      {#if upNext.length}
        <ol class="queue">
          {#each upNext as item (item.id)}
            <li>
              {#if item.track.art_hash}
                <img src={art(item.track.art_hash, 128)} alt="" />
              {:else}
                <div class="no-art small">♪</div>
              {/if}
              <div class="text">
                <div class="title">{item.track.title}</div>
                <div class="sub">{artist(item.track)}</div>
                <div class="sub who">
                  {item.username}{item.backers.length ? ` +${item.backers.length}` : ''}
                </div>
              </div>
            </li>
          {/each}
        </ol>
        {#if more}<p class="more">and {more} more</p>{/if}
      {:else if nowPlaying.up_next}
        <ol class="queue">
          <li>
            {#if nowPlaying.up_next.track.art_hash}
              <img src={art(nowPlaying.up_next.track.art_hash, 128)} alt="" />
            {:else}
              <div class="no-art small">♪</div>
            {/if}
            <div class="text">
              <div class="title">{nowPlaying.up_next.track.title}</div>
              <div class="sub">{artist(nowPlaying.up_next.track)}</div>
            </div>
          </li>
        </ol>
        <p class="more">No requests yet. Yours could be next.</p>
      {:else}
        <p class="more">No requests yet. Yours could be next.</p>
      {/if}

      <div class="join">
        <img src="/api/join/qr.svg" alt="QR code to join" />
        <div>
          <div class="join-title">Add a song</div>
          <div class="join-steps">
            Scan with your phone camera{guestMode ? ', type your name,' : ' and sign in,'} then pick a song.
          </div>
          {#if joinUrl}<div class="url">{joinUrl.replace(/^https?:\/\//, '')}</div>{/if}
        </div>
      </div>
    </aside>
  {/if}

  <button class="fullscreen" onclick={toggleFullscreen}>Full screen</button>
</div>

<style>
  .tv {
    position: fixed;
    inset: 0;
    display: grid;
    grid-template-columns: minmax(0, 1.9fr) minmax(0, 1fr);
    gap: 3vw;
    padding: 4vh 3vw;
    overflow: hidden;
    color: #fff;
    background: #0d0b14;
    font-size: clamp(14px, 1.6vw, 40px);
  }

  .tv.idle-cursor,
  .tv.idle-cursor * {
    cursor: none;
  }

  .backdrop {
    position: absolute;
    inset: -10%;
    background-size: cover;
    background-position: center;
    filter: blur(60px) saturate(1.3);
    opacity: 0.55;
  }

  .shade {
    position: absolute;
    inset: 0;
    background: linear-gradient(90deg, rgb(10 8 16 / 0.55), rgb(10 8 16 / 0.85));
  }

  .now,
  .side,
  .message {
    position: relative;
    min-width: 0;
  }

  .now {
    display: flex;
    align-items: center;
    gap: 3vw;
  }

  .cover {
    flex: none;
    width: min(42vh, 30vw);
    aspect-ratio: 1;
    border-radius: 1.2vw;
    overflow: hidden;
    box-shadow: 0 2vh 6vh rgb(0 0 0 / 0.5);
    background: rgb(255 255 255 / 0.08);
  }

  .cover img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .no-art {
    display: grid;
    place-items: center;
    width: 100%;
    height: 100%;
    font-size: 8vw;
    color: rgb(255 255 255 / 0.4);
    background: rgb(255 255 255 / 0.08);
  }

  .no-art.small {
    width: 3.2em;
    height: 3.2em;
    font-size: 1em;
    border-radius: 0.4em;
  }

  .info {
    min-width: 0;
    flex: 1;
  }

  .label {
    font-size: 0.8em;
    letter-spacing: 0.12em;
    text-transform: uppercase;
    color: #c9b3ff;
  }

  h1 {
    margin: 0.2em 0 0.15em;
    font-size: 2.6em;
    line-height: 1.1;
    overflow-wrap: anywhere;
    display: -webkit-box;
    -webkit-line-clamp: 3;
    line-clamp: 3;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }

  .artist {
    margin: 0;
    font-size: 1.5em;
    color: rgb(255 255 255 / 0.9);
  }

  .album {
    margin: 0.2em 0 0;
    font-size: 1em;
    color: rgb(255 255 255 / 0.6);
  }

  .progress {
    height: 0.35em;
    margin-top: 1.6em;
    border-radius: 1em;
    background: rgb(255 255 255 / 0.2);
    overflow: hidden;
  }

  .progress div {
    height: 100%;
    background: #c9b3ff;
    transition: width 0.25s linear;
  }

  .times {
    display: flex;
    justify-content: space-between;
    margin-top: 0.4em;
    font-size: 0.8em;
    color: rgb(255 255 255 / 0.6);
    font-variant-numeric: tabular-nums;
  }

  .by {
    margin: 1.2em 0 0;
    font-size: 1em;
    color: rgb(255 255 255 / 0.75);
  }

  .side {
    display: flex;
    flex-direction: column;
    min-height: 0;
  }

  h2 {
    margin: 0 0 0.6em;
    font-size: 0.8em;
    letter-spacing: 0.12em;
    text-transform: uppercase;
    color: #c9b3ff;
  }

  .queue {
    margin: 0;
    padding: 0;
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 0.8em;
    min-height: 0;
    overflow: hidden;
  }

  .queue li {
    display: flex;
    align-items: center;
    gap: 0.7em;
  }

  .queue img {
    flex: none;
    width: 3.2em;
    height: 3.2em;
    border-radius: 0.4em;
    object-fit: cover;
  }

  .text {
    min-width: 0;
  }

  .title,
  .sub {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .title {
    font-weight: 600;
  }

  .sub {
    font-size: 0.8em;
    color: rgb(255 255 255 / 0.65);
  }

  .who {
    color: #c9b3ff;
  }

  .more {
    margin: 0.8em 0 0;
    font-size: 0.85em;
    color: rgb(255 255 255 / 0.6);
  }

  .join {
    display: flex;
    align-items: center;
    gap: 1em;
    margin-top: auto;
    padding: 1em;
    border-radius: 1em;
    background: rgb(255 255 255 / 0.1);
  }

  .join img {
    flex: none;
    width: min(9em, 22vh);
    aspect-ratio: 1;
    border-radius: 0.5em;
    background: #fff;
  }

  .join-title {
    font-size: 1.3em;
    font-weight: 700;
  }

  .join-steps {
    margin-top: 0.3em;
    font-size: 0.85em;
    color: rgb(255 255 255 / 0.8);
  }

  .url {
    margin-top: 0.5em;
    font-size: 0.85em;
    color: #c9b3ff;
    overflow-wrap: anywhere;
  }

  .message {
    display: flex;
    flex-direction: column;
    justify-content: center;
    grid-column: 1;
  }

  .message h1 {
    font-size: 5em;
    color: #c9b3ff;
  }

  .message p {
    font-size: 1.4em;
    color: rgb(255 255 255 / 0.8);
  }

  .fullscreen {
    position: absolute;
    right: 1.5vw;
    top: 1.5vh;
    padding: 0.4em 0.9em;
    border: 0;
    border-radius: 2em;
    background: rgb(255 255 255 / 0.15);
    color: #fff;
    font-size: 0.7em;
    opacity: 1;
    transition: opacity 0.3s;
  }

  .idle-cursor .fullscreen {
    opacity: 0;
  }

  /* Portrait screens (tablets on a stand): stack. */
  @media (max-aspect-ratio: 1/1) {
    .tv {
      grid-template-columns: 1fr;
      grid-template-rows: auto 1fr;
      font-size: clamp(14px, 3vw, 40px);
    }

    .cover {
      width: 34vw;
    }

    h1 {
      font-size: 1.9em;
    }

    .artist {
      font-size: 1.2em;
    }
  }
</style>
