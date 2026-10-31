<script lang="ts">
  // One song in a list: tap to play from here, heart to like, menu for
  // the rest.
  import Art from '../lib/Art.svelte'
  import { duration } from '../lib/format'
  import Icon from '../lib/Icon.svelte'
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
</script>

<div class="row" class:playing>
  <button class="main" onclick={onplay}>
    {#if art}
      <Art hash={track.art_hash} size={44} />
    {:else}
      <span class="number muted">{number ?? ''}</span>
    {/if}
    <span class="text">
      <span class="title">{track.title}</span>
      <span class="muted small">{[track.artist, track.album].filter(Boolean).join(' · ')}</span>
    </span>
    <span class="muted small time">{duration(track.duration_ms)}</span>
  </button>
  <button
    class="icon"
    class:liked
    aria-label={liked ? `Unlike ${track.title}` : `Like ${track.title}`}
    aria-pressed={liked}
    onclick={() => toggleLike('track', track.id)}
  >
    <Icon name={liked ? 'heart' : 'heart-outline'} size={20} />
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
</style>
