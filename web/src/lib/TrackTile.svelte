<script lang="ts">
  // A song as a grid tile: tap the cover or title to queue it.
  import Art from './Art.svelte'
  import Credits from './Credits.svelte'
  import Icon from './Icon.svelte'
  import { tapTrack, toggleRequest, trackStatus } from './state.svelte'
  import type { Track } from './types'

  let { track }: { track: Track } = $props()
  const status = $derived(trackStatus(track.id))
</script>

<div class="tile">
  <div class="cover">
    <button class="art" onclick={() => tapTrack(track)} aria-label="Queue {track.title}">
      <Art hash={track.art_hash} size={160} round={10} />
    </button>
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
  <button class="title ellipsis" onclick={() => tapTrack(track)}>{track.title}</button>
  <span class="muted small ellipsis"><Credits {track} /></span>
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

  .art {
    width: 100%;
    height: 100%;
    min-height: 0;
    padding: 0;
    background: transparent;
  }

  .cover :global(.art) {
    width: 100% !important;
    height: 100% !important;
  }

  .title {
    display: block;
    min-height: 0;
    padding: 0;
    background: transparent;
    color: var(--text);
    text-align: left;
    font: inherit;
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
