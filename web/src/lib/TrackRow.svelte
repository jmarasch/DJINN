<script lang="ts">
  import Counts from './Counts.svelte'
  // One song: tap its picture or title to queue it; the artist and album
  // names go to their pages.
  import Art from './Art.svelte'
  import Credits from './Credits.svelte'
  import { duration } from './format'
  import Icon from './Icon.svelte'
  import LyricsMark from './LyricsMark.svelte'
  import { tapTrack, toggleRequest, trackStatus } from './state.svelte'
  import TrackMenu from './TrackMenu.svelte'
  import Heart from './Heart.svelte'
  import LikeCount from './LikeCount.svelte'
  import { t } from './i18n.svelte'
  import type { Track } from './types'

  let {
    track,
    showArt = true,
    showAlbum = true,
    number = null,
    note = null,
    rich = false,
  }: {
    track: Track
    showArt?: boolean
    showAlbum?: boolean
    number?: number | null
    /** Extra text after the duration, like "12 requests". */
    note?: string | null
    /** In the library's song list: on wide screens a neon card with the genres and counts beside it. */
    rich?: boolean
  } = $props()

  const status = $derived(trackStatus(track.id))
</script>

<div class="row" class:rich>
  {#if showArt}
    <button class="art" onclick={() => tapTrack(track)} aria-label="Queue {track.title}" tabindex="-1">
      <Art hash={track.art_hash} size={44} />
    </button>
  {:else if number != null}
    <span class="num">{number}</span>
  {/if}
  <div class="text">
    <div class="title-line">
      <button class="title ellipsis" onclick={() => tapTrack(track)}>{track.title}</button>
      <LyricsMark lyrics={track.lyrics} />
    </div>
    <div class="sub ellipsis">
      <Credits {track} />
      {#if showAlbum && track.album}
        ·
        {#if track.album_id}<a href="#/album/{track.album_id}">{track.album}</a>{:else}{track.album}{/if}
      {/if}
      {#if track.duration_ms}<span class="meta"> · {duration(track.duration_ms)}</span>{/if}
      {#if note}<span class="meta"> · {note}</span>{/if}
      <span class:narrow-only={rich}><Counts plays={track.plays} likes={track.likes} /></span>
      <span class:narrow-only={rich}>
        {#each track.genres ?? [] as genre, i (genre)}
          <span class="meta">{i === 0 ? ' · ' : ', '}</span><a class="genre" href="#/genre/{encodeURIComponent(genre)}"
            >{genre}</a
          >
        {/each}
      </span>
    </div>
  </div>
  {#if rich}
    {#if track.plays}<span class="plays muted small">{t('count.plays', { n: track.plays })}</span>{/if}
    <span class="wide-only"><LikeCount kind="track" id={track.id} name={track.title} likes={track.likes} /></span>
    <span class="narrow-only"><Heart kind="track" id={track.id} name={track.title} /></span>
    <span class="genre-chips">
      {#each (track.genres ?? []).slice(0, 3) as genre (genre)}
        <a class="genre-chip" href="#/genre/{encodeURIComponent(genre)}">{genre}</a>
      {/each}
    </span>
  {:else}
    <Heart kind="track" id={track.id} name={track.title} />
  {/if}
  <TrackMenu {track} />
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

  .genre-chips,
  .plays,
  .wide-only {
    display: none;
  }

  /* The library's song list on wide screens: a neon card per song. */
  @media (min-width: 900px) {
    .row.rich {
      margin-bottom: 8px;
      padding: 6px 12px 6px 8px;
      border: 1px solid var(--edge);
      border-radius: 14px;
      background: var(--card-bg);
      box-shadow: var(--glow);
    }

    .row.rich:hover {
      border-color: var(--edge-strong);
      box-shadow: var(--glow-strong);
    }

    .narrow-only {
      display: none;
    }

    /* The title and its counts stay together; the genres go to the far end. */
    .row.rich .text {
      flex: none;
      width: 340px;
    }

    .row.rich .genre-chips {
      display: flex;
      flex: 1;
      justify-content: flex-end;
      gap: 6px;
    }

    .row.rich .plays {
      display: inline;
      flex: none;
      white-space: nowrap;
    }

    .row.rich .wide-only {
      display: inline;
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

  .title-line {
    display: flex;
    align-items: center;
    min-width: 0;
  }

  .title {
    display: block;
    min-width: 0;
    max-width: 100%;
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

  .genre {
    color: var(--accent);
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
