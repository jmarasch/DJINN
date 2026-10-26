<script lang="ts">
  import { onMount } from 'svelte'
  import { api } from '../lib/api'
  import Art from '../lib/Art.svelte'
  import Credits from '../lib/Credits.svelte'
  import { byline, duration } from '../lib/format'
  import Icon from '../lib/Icon.svelte'
  import {
    app,
    cancelRequest,
    currentPosition,
    isAdmin,
    myRequest,
    signInAsHost,
    toast,
  } from '../lib/state.svelte'
  import { cycleTheme, theme } from '../lib/theme.svelte'

  const current = $derived(app.nowPlaying.current)
  const next = $derived(app.nowPlaying.up_next)
  /** "Up next" is your own request, so you can cancel it. */
  const nextMine = $derived(next ? myRequest(next.track.id) : null)
  const total = $derived(current?.track.duration_ms ?? 0)
  let position = $state(0)
  let volume = $state(app.player.volume)
  let crossfade = $state(app.player.crossfade_secs)

  onMount(() => {
    let frame = 0
    const tick = () => {
      position = Math.min(currentPosition(), total || Infinity)
      frame = requestAnimationFrame(tick)
    }
    tick()
    return () => cancelAnimationFrame(frame)
  })

  // Follow changes made elsewhere.
  $effect(() => {
    volume = app.player.volume
  })
  $effect(() => {
    crossfade = app.player.crossfade_secs
  })

  let crossfadeTimer: ReturnType<typeof setTimeout>
  function onCrossfade() {
    clearTimeout(crossfadeTimer)
    crossfadeTimer = setTimeout(() => control(() => api.setCrossfade(crossfade)), 200)
  }

  let volumeTimer: ReturnType<typeof setTimeout>
  function onVolume() {
    clearTimeout(volumeTimer)
    volumeTimer = setTimeout(() => control(() => api.setVolume(volume)), 120)
  }

  async function control(action: () => Promise<void>) {
    try {
      await action()
    } catch (e) {
      toast(e instanceof Error ? e.message : 'That did not work', true)
    }
  }

  const themeLabel = $derived({ auto: 'Auto', light: 'Light', dark: 'Dark' }[theme.mode])
</script>

<div class="top">
  {#if app.player.playlist}
    <a class="source" href="#/playlist/{app.player.playlist.id}" class:plain={!isAdmin()}>
      <Icon name={app.player.playlist.shuffle ? 'shuffle' : 'ordered'} size={16} />
      {app.player.playlist.name}
    </a>
  {:else}
    <span></span>
  {/if}
  <button class="theme" onclick={cycleTheme} aria-label="Theme: {themeLabel}" title="Theme: {themeLabel}">
    <Icon name="theme" size={18} />
    {themeLabel}
  </button>
</div>

<section class="now">
  {#if current}
    <div class="cover">
      <Art hash={current.track.art_hash} size={320} round={14} />
    </div>
    <h1 class="title">{current.track.title}</h1>
    <p class="artist">
      {#if current.track.credits.length || current.track.artist}
        <Credits track={current.track} />
      {:else}
        Unknown artist
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
      {#if app.player.state !== 'playing'}
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
    <div class="audio">
      <label class="volume">
        <span class="row-label">Volume <span class="muted">{volume}</span></span>
        <input type="range" min="0" max="100" bind:value={volume} oninput={onVolume} />
      </label>
      <label class="volume">
        <span class="row-label">
          Crossfade <span class="muted">{crossfade ? `${crossfade} s` : 'Off (gapless)'}</span>
        </span>
        <input type="range" min="0" max="12" step="1" bind:value={crossfade} oninput={onCrossfade} />
      </label>
      <p class="small muted">
        {#if app.output.error}
          <span class="error">Output problem: {app.output.error}</span>
        {:else if app.output.device}
          {app.output.device} · {(app.output.sample_rate ?? 0) / 1000} kHz
        {:else}
          No audio output yet
        {/if}
      </p>
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
        {#if nextMine}
          <button class="cancel" onclick={() => nextMine && cancelRequest(nextMine.id)}>Cancel</button>
        {/if}
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
  .top {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    min-height: 40px;
  }

  .source {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    padding: 4px 10px;
    border-radius: 999px;
    background: var(--surface);
    color: var(--muted);
    font-size: 0.8125rem;
    text-decoration: none;
  }

  .source.plain {
    pointer-events: none;
  }

  .theme {
    min-height: 34px;
    padding: 0 10px;
    border-radius: 999px;
    background: var(--surface);
    color: var(--muted);
    font-size: 0.8125rem;
  }

  .now {
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
    gap: 6px;
    padding-top: 4px;
  }

  .cover {
    width: min(320px, 74vw);
    aspect-ratio: 1;
    margin-bottom: 14px;
  }

  .cover :global(.art) {
    width: 100% !important;
    height: 100% !important;
    box-shadow: 0 16px 48px rgb(0 0 0 / 0.35);
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
    padding: 36px 0 16px;
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

  .audio {
    width: 100%;
    margin-top: 8px;
    text-align: left;
  }

  .volume {
    gap: 2px;
  }

  .row-label {
    display: flex;
    justify-content: space-between;
    font-size: 0.875rem;
  }

  .volume input {
    width: 100%;
  }

  .audio p {
    margin: 2px 0 0;
  }

  .next {
    width: 100%;
    margin-top: 20px;
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
    flex: 1;
    min-width: 0;
  }

  .cancel {
    flex: none;
    min-height: 36px;
    padding: 0 12px;
    font-size: 0.875rem;
  }

  .host-link {
    margin-top: 28px;
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
</style>
