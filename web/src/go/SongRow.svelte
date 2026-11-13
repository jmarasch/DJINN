<script lang="ts">
  // One song in a list: tap to play from here, heart to like, menu for
  // the rest.
  import Art from '../lib/Art.svelte'
  import Counts from '../lib/Counts.svelte'
  import { duration } from '../lib/format'
  import Icon from '../lib/Icon.svelte'
  import LyricsMark from '../lib/LyricsMark.svelte'
  import type { Track } from '../lib/types'
  import { current, likes, toggleLike } from './player.svelte'
  import { ui } from './ui.svelte'

  let {
    track,
    onplay,
    number = null,
    art = true,
  }: { track: Track; onplay: () => void; number?: number | null; art?: boolean } = $props()

  const playing = $derived(current()?.id === track.id)
  const liked = $derived(likes.track.has(track.id))
  /** Taps on the heart since this row showed, so its count follows without a reload. */
  let delta = $state(0)
  const shown = $derived(Math.max(0, (track.likes ?? 0) + delta))
  const line = $derived([track.artist, track.album].filter(Boolean).join(' · '))
</script>

<div class="row" class:playing>
  <button class="main" onclick={onplay}>
    {#if art}
      <Art hash={track.art_hash} size={44} />
    {:else}
      <span class="number muted">{number ?? ''}</span>
    {/if}
    <span class="text">
      <span class="title-line"><span class="title">{track.title}</span><LyricsMark lyrics={track.lyrics} /></span>
      <span class="muted small"
        >{line}<Counts plays={track.plays} likes={track.likes} lead={!!line} /></span
      >
    </span>
    <span class="muted small time">{duration(track.duration_ms)}</span>
  </button>
  <span class="genre-chips">
    {#each (track.genres ?? []).slice(0, 3) as genre (genre)}
      <a class="genre-chip" href="#/genre/{encodeURIComponent(genre)}">{genre}</a>
    {/each}
  </span>
  <button
    class="icon heart"
    class:liked
    aria-label={liked ? `Unlike ${track.title}` : `Like ${track.title}`}
    aria-pressed={liked}
    onclick={() => {
      delta += liked ? -1 : 1
      void toggleLike('track', track.id)
    }}
  >
    <Icon name={liked ? 'heart' : 'heart-outline'} size={20} />
    <span class="count">{shown.toLocaleString()}</span>
  </button>
  <button class="icon" aria-label="More for {track.title}" onclick={() => (ui.menu = track)}>
    <Icon name="more" size={20} />
  </button>
</div>

<style>
  .row {
    display: flex;
    align-items: center;
    gap: 2px;
  }

  .main {
    display: flex;
    flex: 1;
    align-items: center;
    gap: 12px;
    min-width: 0;
    padding: 6px 4px;
    border-radius: 10px;
    background: transparent;
    color: inherit;
    text-align: left;
  }

  .text {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-width: 0;
  }

  .title-line {
    display: flex;
    align-items: center;
    min-width: 0;
  }

  .title,
  .text .small {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .playing .title {
    color: var(--accent);
  }

  .number {
    width: 28px;
    text-align: right;
    font-variant-numeric: tabular-nums;
  }

  .time {
    flex: none;
    font-variant-numeric: tabular-nums;
  }

  .icon {
    flex: none;
    width: 40px;
    min-height: 40px;
    padding: 0;
    background: transparent;
    color: var(--muted);
  }

  .icon.liked {
    color: var(--accent);
  }

  .genre-chips,
  .count {
    display: none;
  }

  /* On wide screens each song is a neon card, with its genres at the far end. */
  @media (min-width: 900px) {
    .row {
      margin-bottom: 8px;
      padding: 2px 8px;
      border: 1px solid var(--edge);
      border-radius: 14px;
      background: var(--card-bg);
      box-shadow: var(--glow);
    }

    .row:hover {
      border-color: var(--edge-strong);
      box-shadow: var(--glow-strong);
    }

    .genre-chips {
      display: flex;
      flex: none;
      gap: 6px;
    }

    .genre-chip {
      padding: 4px 10px;
      border: 1px solid var(--edge);
      border-radius: 999px;
      background: var(--surface-2);
      color: var(--muted);
      font-size: 0.8125rem;
      text-decoration: none;
      white-space: nowrap;
    }

    .genre-chip:hover {
      border-color: var(--edge-strong);
      color: var(--text);
    }

    .heart {
      display: inline-flex;
      align-items: center;
      gap: 5px;
      width: auto;
      padding: 0 6px;
      font-size: 0.875rem;
      font-variant-numeric: tabular-nums;
    }

    .count {
      display: inline;
    }
  }
</style>
