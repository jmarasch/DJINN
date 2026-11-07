<script lang="ts">
  import Art from '../../lib/Art.svelte'
  import { duration } from '../../lib/format'
  import Icon from '../../lib/Icon.svelte'
  import { toast } from '../../lib/state.svelte'
  import {
    clearAll,
    clearUpcoming,
    moveInQueue,
    current,
    cycleRepeat,
    isAdded,
    jumpTo,
    likes,
    next,
    player,
    previous,
    removeAt,
    seek,
    setShuffle,
    toggle,
    toggleLike,
    livePosition,
  } from '../player.svelte'
  import { ui } from '../ui.svelte'
  import { api } from '../../lib/api'
  import LyricsView from '../../lib/LyricsView.svelte'
  import type { Lyrics } from '../../lib/types'

  const song = $derived(current())

  // Lyrics under the controls, opened or closed on this device.
  let lyricsOpen = $state(readLyricsOpen())
  let lyrics = $state<Lyrics | null | undefined>(undefined)
  function readLyricsOpen(): boolean {
    try {
      return localStorage.getItem('go-lyrics-open') === '1'
    } catch {
      return false
    }
  }
  function toggleLyrics() {
    lyricsOpen = !lyricsOpen
    try {
      localStorage.setItem('go-lyrics-open', lyricsOpen ? '1' : '0')
    } catch {
      // Not remembered.
    }
  }
  $effect(() => {
    const id = song?.id
    if (!lyricsOpen || id == null) return
    lyrics = undefined
    let live = true
    api
      .trackLyrics(id)
      .then((found) => {
        if (live) lyrics = found
      })
      .catch(() => {
        if (live) lyrics = null
      })
    return () => {
      live = false
    }
  })

  function clear(clearing: () => () => void) {
    toast('Queue cleared', false, { label: 'Undo', run: clearing() })
  }
  const upNext = $derived(player.queue.slice(player.index + 1))

  // Dragging a song in Up next by its handle: the row follows the finger, and
  // the queue only changes when it's let go.
  let dragging = $state<number | null>(null)
  let dragY = $state(0)
  let dragStart = 0
  let rowHeight = 56

  function dragBegin(e: PointerEvent, i: number) {
    const handle = e.currentTarget as HTMLElement
    handle.setPointerCapture(e.pointerId)
    rowHeight = handle.closest('.queued')?.getBoundingClientRect().height ?? rowHeight
    dragStart = e.clientY
    dragY = 0
    dragging = i
  }

  function dragMove(e: PointerEvent) {
    if (dragging !== null) dragY = e.clientY - dragStart
  }

  function dragEnd() {
    if (dragging === null) return
    const from = dragging
    const places = Math.round(dragY / rowHeight)
    dragging = null
    dragY = 0
    if (places) moveInQueue(player.index + 1 + from, player.index + 1 + from + places)
  }

  /** The same with arrow keys, for anyone not using a pointer. */
  function dragKey(e: KeyboardEvent, i: number) {
    const by = e.key === 'ArrowUp' ? -1 : e.key === 'ArrowDown' ? 1 : 0
    if (!by) return
    e.preventDefault()
    moveInQueue(player.index + 1 + i, player.index + 1 + i + by)
  }
  const time = (s: number) => duration(s * 1000)
</script>

<div class="playing-page">
  <h1>Now playing</h1>
  {#if song}
    <section class="now">
      <Art hash={song.art_hash} size={300} round={12} />
      <div class="names">
        <div class="big-title">{song.title}</div>
        <div class="muted">
          {#if song.artist_id}<a href="#/artist/{song.artist_id}">{song.artist}</a>{:else}{song.artist ?? ''}{/if}
          {#if song.album_id}
            · <a href="#/album/{song.album_id}">{song.album}</a>
          {/if}
        </div>
      </div>
      <input
        class="seek"
        type="range"
        min="0"
        max={player.duration || 0}
        step="1"
        value={player.position}
        oninput={(e) => seek(Number(e.currentTarget.value))}
        aria-label="Position"
      />
      <div class="times muted small"><span>{time(player.position)}</span><span>{time(player.duration)}</span></div>
      <div class="controls">
        <button
          class="icon"
          class:on={player.shuffle}
          aria-pressed={player.shuffle}
          aria-label="Shuffle"
          onclick={() => setShuffle(!player.shuffle)}
        >
          <Icon name="shuffle" />
        </button>
        <button class="icon" aria-label="Previous" onclick={previous}><Icon name="previous" size={30} /></button>
        <button class="icon play" aria-label={player.playing ? 'Pause' : 'Play'} onclick={toggle}>
          <Icon name={player.playing ? 'pause' : 'play'} size={36} />
        </button>
        <button class="icon" aria-label="Next" onclick={next}><Icon name="skip" size={30} /></button>
        <button
          class="icon"
          class:on={player.repeat !== 'off'}
          aria-label="Repeat: {player.repeat}"
          onclick={cycleRepeat}
        >
          <Icon name={player.repeat === 'one' ? 'repeat-one' : 'repeat'} />
        </button>
      </div>
      <div class="extras">
        <button class="small-button" class:on={likes.track.has(song.id)} onclick={() => toggleLike('track', song.id)}>
          <Icon name={likes.track.has(song.id) ? 'heart' : 'heart-outline'} size={18} />
          {likes.track.has(song.id) ? 'Liked' : 'Like'}
        </button>
        <button class="small-button" class:on={lyricsOpen} aria-pressed={lyricsOpen} onclick={toggleLyrics}>
          <Icon name="lyrics" size={18} /> Lyrics
        </button>
        <button class="small-button" onclick={() => (ui.menu = song)}>More</button>
        {#if player.downloadUrl}<a class="button small-button" href={player.downloadUrl} download>Download</a>{/if}
      </div>
      {#if lyricsOpen}
        <LyricsView {lyrics} position={livePosition} />
      {/if}
    </section>

    <section class="queue">
      <div class="queue-head">
        <h2>Up next{#if player.radio}{' '}<span class="muted small">(radio)</span>{/if}</h2>
        {#if upNext.length}
          <button class="small-button" onclick={() => clear(clearUpcoming)}>Clear upcoming</button>
          <button class="small-button" onclick={() => clear(clearAll)}>Clear all</button>
        {/if}
      </div>
      {#each upNext as track, i (`${track.id}-${player.index + 1 + i}`)}
        <div class="queued" class:dragging={dragging === i} style:transform={dragging === i ? `translateY(${dragY}px)` : null}>
          <button
            class="icon drag"
            aria-label="Move {track.title}"
            title="Drag to move, or use the arrow keys"
            onpointerdown={(e) => dragBegin(e, i)}
            onpointermove={dragMove}
            onpointerup={dragEnd}
            onpointercancel={dragEnd}
            onkeydown={(e) => dragKey(e, i)}
          >
            <Icon name="drag" size={18} />
          </button>
          <button class="queued-song" onclick={() => jumpTo(player.index + 1 + i)}>
            <span class="title">{track.title}</span>
            <span class="muted small">{track.artist ?? ''}{#if isAdded(track)}{' · '}<span class="added">Added by you</span>{/if}</span>
          </button>
          <button class="icon" aria-label="Remove {track.title} from the queue" onclick={() => removeAt(player.index + 1 + i)}>
            <Icon name="close" size={18} />
          </button>
        </div>
      {:else}
        <p class="muted">Nothing after this{player.repeat === 'all' ? '; the queue starts over' : ''}.</p>
      {/each}
    </section>
  {:else}
    <section class="empty">
      <img src="/images/empty-record.webp" alt="" />
      <h2>Nothing playing yet</h2>
      <p class="muted">Find something in the library to start listening.</p>
      <a class="button primary" href="#/search">Explore the library</a>
    </section>
  {/if}
</div>

<style>
  .playing-page {
    position: relative;
    isolation: isolate;
    min-height: calc(100dvh - 140px);
    padding: 18px 0 36px;
  }

  .playing-page::before {
    content: '';
    position: absolute;
    z-index: -1;
    top: 0;
    left: 0;
    right: 0;
    height: 440px;
    background: linear-gradient(to bottom, transparent, var(--bg)), url('/images/listening-waves.webp') center top / cover no-repeat;
    opacity: 0.55;
    pointer-events: none;
  }

  h1 {
    margin: 0 0 20px;
    font-size: 1.4rem;
  }

  .now {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 12px;
    max-width: 540px;
    margin: 0 auto;
  }

  .now :global(.art) {
    width: min(300px, 100%) !important;
    height: auto !important;
    aspect-ratio: 1;
  }

  .names {
    width: 100%;
    text-align: center;
  }

  .big-title {
    font-size: 1.35rem;
    font-weight: 600;
  }

  .seek {
    width: 100%;
    accent-color: var(--accent);
  }

  .times {
    display: flex;
    justify-content: space-between;
    width: 100%;
    margin-top: -8px;
    font-variant-numeric: tabular-nums;
  }

  .controls, .extras {
    display: flex;
    align-items: center;
    justify-content: center;
    flex-wrap: wrap;
    gap: 8px;
  }

  .icon {
    flex: none;
    width: 44px;
    min-height: 44px;
    padding: 0;
    background: transparent;
    color: var(--text);
  }

  .icon.on, .small-button.on, .added {
    color: var(--accent);
  }

  .play {
    width: 64px;
    min-height: 64px;
    border-radius: 999px;
    background: var(--accent);
    color: var(--accent-text);
  }

  .queue {
    max-width: 540px;
    margin: 30px auto 0;
  }

  h2 {
    margin: 0 0 8px;
    font-size: 1.1rem;
  }

  .queue-head {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    margin-bottom: 8px;
  }

  .queue-head h2 {
    flex: 1;
    margin: 0;
  }

  .queued {
    display: flex;
    align-items: center;
    border-bottom: 1px solid var(--line);
  }

  .queued.dragging {
    position: relative;
    z-index: 2;
    background: var(--surface-2);
    box-shadow: 0 4px 14px rgb(0 0 0 / 35%);
  }

  /* A finger on the handle moves the song, not the page. */
  .drag {
    touch-action: none;
    cursor: grab;
    color: var(--muted);
  }

  .queued-song {
    display: flex;
    flex: 1;
    flex-direction: column;
    align-items: flex-start;
    min-width: 0;
    padding: 10px 4px;
    background: transparent;
    color: inherit;
    text-align: left;
  }

  .title {
    overflow: hidden;
    max-width: 100%;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
  }

  .empty img {
    width: min(240px, 70vw);
    height: auto;
  }
</style>
