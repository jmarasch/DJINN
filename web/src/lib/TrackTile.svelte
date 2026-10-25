<script lang="ts">
  // A song as a grid tile: cover, title, artist, and the queue button.
  import Art from './Art.svelte'
  import Icon from './Icon.svelte'
  import { toggleRequest, trackStatus } from './state.svelte'
  import type { Track } from './types'

  let { track }: { track: Track } = $props()
  const status = $derived(trackStatus(track.id))
</script>

<div class="tile">
  <div class="cover">
    <Art hash={track.art_hash} size={160} round={10} />
    <button
      class="queue"
      class:done={status !== null}
      disabled={status !== null && status !== 'mine'}
      onclick={() => toggleRequest(track.id)}
      aria-label={status === 'mine' ? `Cancel your request for ${track.title}` : `Queue ${track.title}`}
    >
      <Icon
        name={status === 'playing' ? 'play' : status === 'pending' ? 'clock' : status ? 'check' : 'plus'}
        size={20}
      />
    </button>
  </div>
  <span class="ellipsis">{track.title}</span>
  <span class="muted small ellipsis">{track.artist ?? ''}</span>
</div>

<style>
  .tile {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
  }

  .cover {
    position: relative;
    aspect-ratio: 1;
  }

  .cover :global(.art) {
    width: 100% !important;
    height: 100% !important;
  }

  .queue {
    position: absolute;
    right: 6px;
    bottom: 6px;
    width: 40px;
    min-height: 40px;
    height: 40px;
    padding: 0;
    border-radius: 50%;
    background: var(--accent);
    color: var(--accent-text);
    box-shadow: 0 2px 10px rgb(0 0 0 / 0.4);
  }

  .queue.done {
    background: var(--surface-3);
    color: var(--ok);
    opacity: 1;
  }
</style>
