<script lang="ts">
  import Art from '../../lib/Art.svelte'
  import { duration } from '../../lib/format'
  import Icon from '../../lib/Icon.svelte'
  import {
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
  } from '../player.svelte'
  import { ui } from '../ui.svelte'

  const song = $derived(current())
  const upNext = $derived(player.queue.slice(player.index + 1))
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
        <button class="small-button" onclick={() => (ui.menu = song)}>More</button>
        {#if player.downloadUrl}<a class="button small-button" href={player.downloadUrl} download>Download</a>{/if}
      </div>
    </section>

    <section class="queue">
      <h2>Up next{#if player.radio}{' '}<span class="muted small">(radio)</span>{/if}</h2>
      {#each upNext as track, i (`${track.id}-${player.index + 1 + i}`)}
        <div class="queued">
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

  .queued {
    display: flex;
    align-items: center;
    border-bottom: 1px solid var(--line);
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
