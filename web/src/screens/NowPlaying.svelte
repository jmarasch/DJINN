<script lang="ts">
  import { onMount } from 'svelte'
  import { api } from '../lib/api'
  import Art from '../lib/Art.svelte'
  import { byline, duration } from '../lib/format'
  import Icon from '../lib/Icon.svelte'
  import { app, currentPosition, isAdmin, signInAsHost, toast } from '../lib/state.svelte'

  const current = $derived(app.nowPlaying.current)
  const next = $derived(app.nowPlaying.up_next)
  const total = $derived(current?.track.duration_ms ?? 0)
  let position = $state(0)

  onMount(() => {
    let frame = 0
    const tick = () => {
      position = Math.min(currentPosition(), total || Infinity)
      frame = requestAnimationFrame(tick)
    }
    tick()
    return () => cancelAnimationFrame(frame)
  })

  async function control(action: () => Promise<void>) {
    try {
      await action()
    } catch (e) {
      toast(e instanceof Error ? e.message : 'That did not work', true)
    }
  }
</script>

<section class="now">
  {#if current}
    <div class="cover">
      <Art hash={current.track.art_hash} size={320} round={14} />
    </div>
    <h1 class="title">{current.track.title}</h1>
    <p class="artist">
      {#if current.track.artist_id}
        <a href="#/artist/{current.track.artist_id}">{current.track.artist}</a>
      {:else}
        {current.track.artist ?? 'Unknown artist'}
      {/if}
      {#if current.track.album_id}
        · <a href="#/album/{current.track.album_id}">{current.track.album}</a>
      {/if}
    </p>
    {#if current.requested_by}
      <p class="badge">Requested by {current.requested_by}</p>
    {/if}
    <div class="progress" role="progressbar" aria-valuenow={position} aria-valuemax={total}>
      <div class="fill" style:width="{total ? (position / total) * 100 : 0}%"></div>
    </div>
    <div class="times">
      <span>{duration(position)}</span>
      <span>{duration(total)}</span>
    </div>
  {:else}
    <div class="idle">
      <Icon name="disc" size={72} />
      <h1>Nothing playing</h1>
      <p class="muted">
        {app.output.error ? `Audio output problem: ${app.output.error}` : 'Find a song and add it to the queue.'}
      </p>
      <a class="button primary" href="#/search">Find a song</a>
    </div>
  {/if}

  {#if isAdmin()}
    <div class="controls">
      {#if app.player.state === 'paused'}
        <button class="round big" aria-label="Play" onclick={() => control(api.play)}>
          <Icon name="play" size={32} />
        </button>
      {:else}
        <button class="round big" aria-label="Pause" onclick={() => control(api.pause)}>
          <Icon name="pause" size={32} />
        </button>
      {/if}
      <button class="round" aria-label="Skip" disabled={!current} onclick={() => control(api.skip)}>
        <Icon name="skip" size={26} />
      </button>
    </div>
  {/if}

  {#if next}
    <div class="next">
      <span class="label">Up next</span>
      <div class="next-row">
        <Art hash={next.track.art_hash} size={40} />
        <div class="text">
          <div class="ellipsis">{next.track.title}</div>
          <div class="muted small ellipsis">
            {byline(next.track.artist)}{next.requested_by ? ` · for ${next.requested_by}` : ''}
          </div>
        </div>
      </div>
    </div>
  {/if}
  {#if !isAdmin()}
    <p class="host-link">
      {#if app.me?.role === 'guest'}You're in as <b>{app.me.display_name}</b>. {/if}
      <button onclick={signInAsHost}>Host sign in</button>
    </p>
  {/if}
</section>

<style>
  .host-link {
    margin-top: 32px;
    color: var(--muted);
    font-size: 0.875rem;
  }

  .host-link button {
    min-height: 32px;
    padding: 0 4px;
    background: transparent;
    color: var(--muted);
    text-decoration: underline;
  }

  .now {
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
    gap: 6px;
    padding-top: 12px;
  }

  .cover {
    width: min(320px, 78vw);
    aspect-ratio: 1;
    margin-bottom: 14px;
  }

  .cover :global(.art) {
    width: 100% !important;
    height: 100% !important;
    box-shadow: 0 16px 48px rgb(0 0 0 / 0.5);
  }

  .title {
    margin: 0;
    font-size: 1.5rem;
  }

  .artist {
    margin: 0;
    color: var(--muted);
  }

  .artist a {
    color: inherit;
  }

  .badge {
    margin: 6px 0 0;
    padding: 4px 12px;
    border-radius: 999px;
    background: var(--surface-2);
    font-size: 0.875rem;
  }

  .progress {
    width: 100%;
    height: 6px;
    margin-top: 18px;
    border-radius: 3px;
    background: var(--surface-2);
    overflow: hidden;
  }

  .fill {
    height: 100%;
    background: var(--accent);
  }

  .times {
    width: 100%;
    display: flex;
    justify-content: space-between;
    color: var(--muted);
    font-size: 0.8125rem;
    font-variant-numeric: tabular-nums;
  }

  .idle {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
    padding: 48px 0 16px;
    color: var(--muted);
  }

  .idle h1 {
    color: var(--text);
    margin: 8px 0 0;
  }

  .controls {
    display: flex;
    align-items: center;
    gap: 20px;
    margin-top: 14px;
  }

  .next {
    width: 100%;
    margin-top: 24px;
    padding: 12px;
    border-radius: 12px;
    background: var(--surface);
    text-align: left;
  }

  .label {
    color: var(--muted);
    font-size: 0.75rem;
    text-transform: uppercase;
    letter-spacing: 0.08em;
  }

  .next-row {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-top: 6px;
  }

  .text {
    min-width: 0;
  }
</style>
