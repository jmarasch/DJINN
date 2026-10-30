<script lang="ts">
  // The player along the bottom, and the Now Playing view with the queue
  // when it's opened.
  import Art from '../lib/Art.svelte'
  import { duration } from '../lib/format'
  import Icon from '../lib/Icon.svelte'
  import Sheet from '../lib/Sheet.svelte'
  import {
    current,
    cycleRepeat,
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
  } from './player.svelte'
  import { ui } from './ui.svelte'

  const song = $derived(current())
  const upNext = $derived(player.queue.slice(player.index + 1))
  const time = (s: number) => duration(s * 1000)
</script>

{#if song}
  <div class="bar">
    <div class="progress" style:width="{player.duration ? (player.position / player.duration) * 100 : 0}%"></div>
    <button class="info" onclick={() => (ui.nowPlaying = true)} aria-label="Open Now Playing">
      <Art hash={song.art_hash} size={44} />
      <span class="text">
        <span class="title">{song.title}</span>
        <span class="muted small">{song.artist ?? ''}</span>
      </span>
    </button>
    <button class="icon" aria-label="Previous" onclick={previous}><Icon name="previous" /></button>
    <button class="icon big" aria-label={player.playing ? 'Pause' : 'Play'} onclick={toggle}>
      <Icon name={player.playing ? 'pause' : 'play'} size={28} />
    </button>
    <button class="icon" aria-label="Next" onclick={next}><Icon name="skip" /></button>
  </div>
{/if}

{#if ui.nowPlaying && song}
  <Sheet title="Now playing" onclose={() => (ui.nowPlaying = false)}>
    <div class="now">
      <Art hash={song.art_hash} size={260} round={12} />
      <div class="names">
        <div class="big-title">{song.title}</div>
        <div class="muted">
          {#if song.artist_id}<a href="#/artist/{song.artist_id}" onclick={() => (ui.nowPlaying = false)}>{song.artist}</a>{:else}{song.artist ?? ''}{/if}
          {#if song.album_id}
            · <a href="#/album/{song.album_id}" onclick={() => (ui.nowPlaying = false)}>{song.album}</a>
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
    </div>

    <h2>Up next{#if player.radio} <span class="muted small">(radio)</span>{/if}</h2>
    {#each upNext as track, i (`${track.id}-${player.index + 1 + i}`)}
      <div class="queued">
        <button class="queued-song" onclick={() => jumpTo(player.index + 1 + i)}>
          <span class="title">{track.title}</span>
          <span class="muted small">{track.artist ?? ''}</span>
        </button>
        <button class="icon" aria-label="Remove {track.title} from the queue" onclick={() => removeAt(player.index + 1 + i)}>
          <Icon name="close" size={18} />
        </button>
      </div>
    {:else}
      <p class="muted">Nothing after this{player.repeat === 'all' ? '; the queue starts over' : ''}.</p>
    {/each}
  </Sheet>
{/if}

<style>
  .bar {
    position: fixed;
    left: 0;
    right: 0;
    bottom: 0;
    z-index: 20;
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 8px 12px calc(8px + env(safe-area-inset-bottom));
    border-top: 1px solid var(--line);
    background: var(--surface);
  }

  .progress {
    position: absolute;
    top: -1px;
    left: 0;
    height: 2px;
    background: var(--accent);
  }

  .info {
    display: flex;
    flex: 1;
    align-items: center;
    gap: 10px;
    min-width: 0;
    padding: 0;
    background: transparent;
    color: inherit;
    text-align: left;
  }

  .text {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .title {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .icon {
    flex: none;
    width: 44px;
    min-height: 44px;
    padding: 0;
    background: transparent;
    color: var(--text);
  }

  .icon.on,
  .small-button.on {
    color: var(--accent);
  }

  .now {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 10px;
  }

  .names {
    width: 100%;
    text-align: center;
  }

  .big-title {
    font-size: 1.2rem;
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
    margin-top: -6px;
    font-variant-numeric: tabular-nums;
  }

  .controls {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .play {
    width: 64px;
    min-height: 64px;
    border-radius: 999px;
    background: var(--accent);
    color: var(--accent-text);
  }

  .extras {
    display: flex;
    flex-wrap: wrap;
    justify-content: center;
    gap: 8px;
  }

  h2 {
    margin: 18px 0 6px;
    font-size: 1rem;
  }

  .queued {
    display: flex;
    align-items: center;
  }

  .queued-song {
    display: flex;
    flex: 1;
    flex-direction: column;
    align-items: flex-start;
    min-width: 0;
    padding: 6px 4px;
    background: transparent;
    color: inherit;
    text-align: left;
  }
</style>
