<script lang="ts">
  // One song: tap its picture or title to queue it; the artist and album
  // names go to their pages.
  import AddToPlaylist from './AddToPlaylist.svelte'
  import Art from './Art.svelte'
  import Credits from './Credits.svelte'
  import { duration } from './format'
  import Icon from './Icon.svelte'
  import { tapTrack, toggleRequest, trackStatus } from './state.svelte'
  import type { Track } from './types'

  let {
    track,
    showArt = true,
    showAlbum = true,
    number = null,
    note = null,
  }: {
    track: Track
    showArt?: boolean
    showAlbum?: boolean
    number?: number | null
    /** Extra text after the duration, like "12 requests". */
    note?: string | null
  } = $props()

  const status = $derived(trackStatus(track.id))
</script>

<div class="row">
  {#if showArt}
    <button class="art" onclick={() => tapTrack(track)} aria-label="Queue {track.title}" tabindex="-1">
      <Art hash={track.art_hash} size={44} />
    </button>
  {:else if number != null}
    <span class="num">{number}</span>
  {/if}
  <div class="text">
    <button class="title ellipsis" onclick={() => tapTrack(track)}>{track.title}</button>
    <div class="sub ellipsis">
      <Credits {track} />
      {#if showAlbum && track.album}
        ·
        {#if track.album_id}<a href="#/album/{track.album_id}">{track.album}</a>{:else}{track.album}{/if}
      {/if}
      {#if track.duration_ms}<span class="meta"> · {duration(track.duration_ms)}</span>{/if}
      {#if note}<span class="meta"> · {note}</span>{/if}
    </div>
  </div>
  <AddToPlaylist kind="track" refId={track.id} label={track.title} compact />
  <button
    class="queue"
    class:done={status !== null && status !== 'queued'}
    class:mine={status === 'mine'}
    disabled={status === 'playing' || status === 'pending'}
    onclick={() => toggleRequest(track.id)}
    aria-label={status === 'mine'
      ? `Take back your request for ${track.title}`
      : status === 'queued'
        ? `Ask for ${track.title} too (moves it up)`
        : status
          ? `Already ${status}`
          : `Queue ${track.title}`}
    title={status === 'mine'
      ? 'You asked for this. Tap to take it back.'
      : status === 'queued'
        ? 'Someone queued this. Ask for it too to move it up.'
        : undefined}
  >
    {#if status === 'playing'}
      <Icon name="play" />
    {:else if status === 'queued'}
      <Icon name="up" />
    {:else if status === 'mine'}
      <Icon name="check" />
    {:else if status === 'pending'}
      <Icon name="clock" />
    {:else}
      <Icon name="plus" />
    {/if}
  </button>
</div>

<style>
  .row {
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: 60px;
    padding: 6px 0;
  }

  .art {
    flex: none;
    min-height: 0;
    padding: 0;
    background: transparent;
    border-radius: 6px;
  }

  .num {
    width: 28px;
    text-align: right;
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }

  .text {
    flex: 1;
    min-width: 0;
  }

  .title {
    display: block;
    width: 100%;
    min-height: 0;
    padding: 0;
    background: transparent;
    color: var(--text);
    text-align: left;
    font: inherit;
  }

  .ellipsis {
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .sub {
    color: var(--muted);
    font-size: 0.875rem;
  }

  .sub a {
    color: inherit;
    text-decoration: none;
  }

  .sub a:hover {
    text-decoration: underline;
  }

  .meta {
    font-variant-numeric: tabular-nums;
  }

  .queue {
    flex: none;
    width: 48px;
    height: 48px;
    padding: 0;
    border-radius: 50%;
    color: var(--accent);
    background: var(--surface-2);
  }

  .queue.done {
    color: var(--ok);
    background: transparent;
  }

  .queue.mine {
    background: color-mix(in srgb, var(--ok) 18%, transparent);
  }
</style>
