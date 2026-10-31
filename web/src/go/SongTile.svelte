<script lang="ts">
  // A song as a grid tile: tap the cover to play it, the dots for more.
  import Art from '../lib/Art.svelte'
  import Icon from '../lib/Icon.svelte'
  import type { Track } from '../lib/types'
  import { current, likes, toggleLike } from './player.svelte'
  import { ui } from './ui.svelte'

  let { track, onplay }: { track: Track; onplay: () => void } = $props()

  const playing = $derived(current()?.id === track.id)
  const liked = $derived(likes.track.has(track.id))
</script>

<div class="tile" class:playing>
  <button class="cover" onclick={onplay} aria-label="Play {track.title}">
    <Art hash={track.art_hash} size={160} round={10} />
  </button>
  <div class="text">
    <button class="title ellipsis" onclick={onplay}>{track.title}</button>
    <div class="actions">
      <button
        class="icon"
        class:liked
        aria-label={liked ? `Unlike ${track.title}` : `Like ${track.title}`}
        aria-pressed={liked}
        onclick={() => toggleLike('track', track.id)}
      >
        <Icon name={liked ? 'heart' : 'heart-outline'} size={18} />
      </button>
      <button class="icon" aria-label="More for {track.title}" onclick={() => (ui.menu = track)}>
        <Icon name="more" size={18} />
      </button>
    </div>
  </div>
  <span class="muted small ellipsis">{track.artist ?? ''}</span>
</div>

<style>
  .tile {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .cover {
    aspect-ratio: 1;
    padding: 0;
    background: transparent;
  }

  .cover :global(.art) {
    width: 100% !important;
    height: 100% !important;
  }

  .text {
    display: flex;
    align-items: center;
    gap: 2px;
    min-width: 0;
  }

  .title {
    flex: 1;
    min-width: 0;
    padding: 0;
    background: transparent;
    color: inherit;
    text-align: left;
  }

  .playing .title {
    color: var(--accent);
  }

  .actions {
    display: flex;
    flex: none;
  }

  .icon {
    width: 32px;
    min-height: 32px;
    padding: 0;
    background: transparent;
    color: var(--muted);
  }

  .icon.liked {
    color: var(--accent);
  }
</style>
