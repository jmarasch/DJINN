<script lang="ts">
  import Art from './Art.svelte'
  import Icon from './Icon.svelte'
  import { byline, duration } from './format'
  import { toggleRequest, trackStatus } from './state.svelte'
  import type { Track } from './types'

  let {
    track,
    showArt = true,
    showAlbum = true,
    number = null,
  }: { track: Track; showArt?: boolean; showAlbum?: boolean; number?: number | null } = $props()

  const status = $derived(trackStatus(track.id))
</script>

<div class="row">
  {#if showArt}
    <Art hash={track.art_hash} size={44} />
  {:else if number != null}
    <span class="num">{number}</span>
  {/if}
  <div class="text">
    <div class="title">{track.title}</div>
    <div class="sub">
      {byline(track.artist, showAlbum ? track.album : null)}
      {#if track.duration_ms}<span class="dur">{duration(track.duration_ms)}</span>{/if}
    </div>
  </div>
  <button
    class="queue"
    class:done={status !== null}
    class:mine={status === 'mine'}
    disabled={status !== null && status !== 'mine'}
    onclick={() => toggleRequest(track.id)}
    aria-label={status === 'mine'
      ? `Cancel your request for ${track.title}`
      : status
        ? `Already ${status}`
        : `Queue ${track.title}`}
    title={status === 'mine' ? 'Queued by you. Tap to cancel.' : undefined}
  >
    {#if status === 'playing'}
      <Icon name="play" />
    {:else if status === 'queued' || status === 'mine'}
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

  .title,
  .sub {
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .sub {
    color: var(--muted);
    font-size: 0.875rem;
  }

  .dur {
    font-variant-numeric: tabular-nums;
  }

  .dur::before {
    content: '· ';
  }

  .queue {
    flex: none;
    width: 48px;
    height: 48px;
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
