<script lang="ts">
  import { onMount } from 'svelte'
  import { api } from '../lib/api'
  import Art from '../lib/Art.svelte'
  import Credits from '../lib/Credits.svelte'
  import { byline, duration } from '../lib/format'
  import Icon from '../lib/Icon.svelte'
  import {
    app,
    askedBy,
    cancelRequest,
    involvement,
    currentPosition,
    isAdmin,
    myRequest,
    signInAsHost,
    toast,
  } from '../lib/state.svelte'
  import { checkPlayHere, playHere, startPlayHere, stopPlayHere } from '../lib/playHere.svelte'
  import { cycleTheme, theme } from '../lib/theme.svelte'

  const current = $derived(app.nowPlaying.current)
  const next = $derived(app.nowPlaying.up_next)
  /** "Up next" is a request you're part of, so you can take yours back. */
  const nextMine = $derived(next ? myRequest(next.track.id) : null)
  const nextQueued = $derived(next ? app.queue.find((q) => q.track.id === next.track.id) : undefined)
  const total = $derived(current?.track.duration_ms ?? 0)
  let position = $state(0)
  let volume = $state(app.player.volume)

  onMount(() => {
    void checkPlayHere()
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

  // Skip votes: whoever requested the song alone can skip it; others vote
  // when the host has skip voting on.
  const soleRequester = $derived(
    !!current && current.requested_by_id === app.me?.id && !current.backers.length,
  )
  const canVote = $derived(!!current && !isAdmin() && (soleRequester || app.player.skip_votes_needed > 0))
  const voted = $derived(app.player.skip_voters.includes(app.me?.id ?? -1))
  const resting = $derived(!soleRequester && app.player.skip_resting.includes(app.me?.id ?? -1))

  async function voteSkip() {
    try {
      const { skipped } = await api.skipVote()
      toast(skipped ? 'Skipped' : 'Vote counted')
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not vote', true)
    }
  }

  const requesters = $derived(
    current?.requested_by ? [current.requested_by, ...current.backers] : [],
  )
  const requesterText = $derived(
    requesters.length < 2
      ? requesters.join('')
      : `${requesters.slice(0, -1).join(', ')} & ${requesters[requesters.length - 1]}`,
  )

  const themeLabel = $derived({ auto: 'Auto', light: 'Light', dark: 'Dark' }[theme.mode])
</script>

<div class="playing-page">
<div class="top">
  {#if app.player.playlist}
    <a
      class="source"
      href={app.player.playlist.id < 0 ? '#/playlists' : `#/playlist/${app.player.playlist.id}`}
      class:plain={!isAdmin()}
    >
      <Icon name={app.player.playlist.shuffle ? 'shuffle' : 'ordered'} size={16} />
      {app.player.playlist.name}
    </a>
  {:else}
    <span></span>
  {/if}
  <div class="display-actions">
    <a class="button" href="/tv" title="Switch to TV display">TV display</a>
    <button class="theme" onclick={cycleTheme} aria-label="Theme: {themeLabel}" title="Theme: {themeLabel}">
      <Icon name="theme" size={18} />
      {themeLabel}
    </button>
  </div>
</div>

<div class="stage">
<section class="now">
  {#if current}
    <div class="cover">
      <Art hash={current.track.art_hash} size={420} round={16} />
    </div>
  {/if}
  <div class="song-info">
  {#if current}
    <span class="label">{app.player.state === 'paused' ? 'Paused' : 'Now playing'}</span>
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
    {#if requesterText}
      <p class="badge">Requested by {requesterText}</p>
    {/if}
    <div class="progress" role="progressbar" aria-valuenow={position} aria-valuemax={total}>
      <div class="fill" style:width="{total ? (position / total) * 100 : 0}%"></div>
    </div>
    <div class="times">
      <span>{duration(position)}</span>
      <span>{duration(total)}</span>
    </div>
    {#if canVote}
      <button class="skip-vote" disabled={voted || resting} onclick={voteSkip}>
        <Icon name="skip" size={18} />
        {#if soleRequester}
          Skip my song
        {:else if resting}
          You can vote to skip again after this song
        {:else if voted}
          Voted to skip · {app.player.skip_votes} of {app.player.skip_votes_needed}
        {:else}
          Vote to skip{app.player.skip_votes ? ` · ${app.player.skip_votes} of ${app.player.skip_votes_needed}` : ''}
        {/if}
      </button>
    {/if}
  {:else}
    <div class="idle">
      <img src="/images/empty-record.webp" alt="" />
      <h1>Nothing playing</h1>
      <p class="muted">
        {app.output.error ? `Audio output problem: ${app.output.error}` : 'Find a song and add it to the queue.'}
      </p>
      <a class="button primary" href="#/search">Find a song</a>
    </div>
  {/if}

  {#if playHere.url}
    <div class="play-here">
      <button
        class:on={playHere.on}
        class:waiting={playHere.on && playHere.waiting}
        aria-pressed={playHere.on}
        title={playHere.on ? 'Stop playing on this device' : 'Play the party on this device'}
        onclick={() => (playHere.on ? stopPlayHere() : startPlayHere())}
      >
        <Icon name="headphones" size={18} />
        Tune In
      </button>
      {#if playHere.on}
        <p class="small muted">
          {playHere.waiting ? 'Connecting… ' : ''}A few seconds behind the speakers. Use this device's volume.
        </p>
      {/if}
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
  </div>
</section>

  <aside class="side">
    <h2>Up next</h2>
    {#if app.queue.length}
      <ol class="queue-list">
        {#each app.queue.slice(0, 6) as item (item.id)}
          {@const mine = myRequest(item.track.id)}
          <li>
            <Art hash={item.track.art_hash} size={56} />
            <div class="text">
              <div class="ellipsis">{item.track.title}</div>
              <div class="muted small ellipsis">{byline(item.track.artist)}</div>
              <div class="small requester">{askedBy(item)}</div>
            </div>
            {#if mine}
              <button class="cancel" onclick={() => cancelRequest(mine.id)}>
                {involvement(mine) === 'backer' ? 'Step back' : 'Cancel'}
              </button>
            {/if}
          </li>
        {/each}
      </ol>
      {#if app.queue.length > 6}<p class="muted small">and {app.queue.length - 6} more</p>{/if}
    {:else if next}
      <div class="next-row">
        <Art hash={next.track.art_hash} size={40} />
        <div class="text">
          <div class="ellipsis">{next.track.title}</div>
          <div class="muted small ellipsis">
            {byline(next.track.artist)}{nextQueued
              ? ` · for ${askedBy(nextQueued)}`
              : next.requested_by
                ? ` · for ${next.requested_by}`
                : ''}
          </div>
        </div>
        {#if nextMine}
          <button class="cancel" onclick={() => nextMine && cancelRequest(nextMine.id)}>
            {involvement(nextMine) === 'backer' ? 'Step back' : 'Cancel'}
          </button>
        {/if}
      </div>
      <p class="muted small">No requests yet. Yours could be next.</p>
    {:else}
      <p class="muted">No requests yet. Yours could be next.</p>
    {/if}

    <div class="join-prompt">
      <img src="/api/join/qr.svg" alt="Scan to add a song" />
      <div>
        <strong>Add a song</strong>
        <p class="small muted">Scan with your phone, then choose from the library.</p>
      </div>
    </div>
  </aside>
</div>

  {#if !isAdmin()}
    <p class="host-link">
      {#if app.me?.role === 'guest'}
        You're in as <b>{app.me.display_name}</b>.
        <button onclick={signInAsHost}>Host sign in</button>
      {:else}
        Signed in as <b>{app.me?.username}</b>. <a href="#/account">Your account</a>
      {/if}
    </p>
  {/if}
</div>

<style>
  .playing-page {
    position: relative;
    isolation: isolate;
    min-height: calc(100dvh - 120px);
  }

  .playing-page::before {
    content: '';
    position: fixed;
    z-index: -1;
    inset: 0;
    background: url('/images/listening-waves.webp') center / cover no-repeat;
    opacity: 0.18;
    pointer-events: none;
  }

  .stage {
    display: grid;
    grid-template-columns: minmax(0, 1.9fr) minmax(280px, 1fr);
    gap: clamp(24px, 3vw, 48px);
    min-height: calc(100dvh - 180px);
    margin-top: 24px;
  }

  .play-here {
    margin-top: 12px;
    text-align: left;
  }

  .play-here button {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 0 16px;
    min-height: 40px;
    border-radius: 999px;
    font-size: 0.875rem;
  }

  .play-here button.on {
    background: var(--accent);
    color: var(--accent-text);
    box-shadow: 0 0 16px var(--accent);
  }

  .play-here button.waiting {
    animation: tuning 1.2s ease-in-out infinite alternate;
  }

  @keyframes tuning {
    to {
      box-shadow: 0 0 4px var(--accent);
      opacity: 0.75;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .play-here button.waiting {
      animation: none;
    }
  }

  .play-here p {
    margin: 6px 0 0;
  }

  .skip-vote {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    margin: 12px 0 0;
    padding: 0 16px;
    min-height: 40px;
    border-radius: 999px;
    font-size: 0.875rem;
  }

  .top {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    min-height: 40px;
  }

  .display-actions {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .display-actions .button {
    min-height: 34px;
    padding: 7px 10px;
    border-radius: 999px;
    background: var(--surface);
    color: var(--muted);
    font-size: 0.8125rem;
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
    align-items: flex-start;
    gap: clamp(20px, 2.5vw, 36px);
    min-width: 0;
    padding-top: 32px;
  }

  .song-info {
    flex: 1;
    min-width: 0;
  }

  .cover {
    flex: none;
    width: min(32vw, 330px);
    aspect-ratio: 1;
  }

  .cover :global(.art) {
    width: 100% !important;
    height: 100% !important;
    box-shadow: 0 16px 48px rgb(0 0 0 / 0.35);
  }

  .title {
    margin: 8px 0 10px;
    font-size: clamp(1.75rem, 3vw, 3rem);
    line-height: 1.1;
    overflow-wrap: anywhere;
  }

  .artist {
    margin: 0;
    color: var(--muted);
  }

  .artist a {
    color: inherit;
  }

  .badge {
    display: inline-block;
    margin: 16px 0 0;
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
    align-items: flex-start;
    gap: 4px;
    padding: 36px 0 16px;
    color: var(--muted);
  }

  .idle img {
    width: min(210px, 58vw);
    height: auto;
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

  .side {
    display: flex;
    flex-direction: column;
    min-width: 0;
    padding: 26px 0 0;
  }

  .side h2 {
    margin: 0 0 22px;
    color: var(--accent);
    font-size: 0.9rem;
    letter-spacing: 0.12em;
    text-transform: uppercase;
  }

  .queue-list {
    display: flex;
    flex-direction: column;
    gap: 18px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .queue-list li {
    display: flex;
    align-items: center;
    gap: 12px;
    min-width: 0;
  }

  .requester {
    color: var(--accent);
  }

  .join-prompt {
    display: flex;
    align-items: center;
    gap: 16px;
    margin-top: auto;
    padding: 16px;
    border-radius: 16px;
    background: color-mix(in srgb, var(--surface) 80%, transparent);
  }

  .join-prompt img {
    flex: none;
    width: clamp(76px, 8vw, 110px);
    aspect-ratio: 1;
    border-radius: 8px;
    background: #fff;
  }

  .join-prompt strong {
    font-size: 1.1rem;
  }

  .join-prompt p {
    margin: 6px 0 0;
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
    margin-top: 0;
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
    margin-top: 22px;
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

  @media (max-width: 950px) {
    .stage {
      grid-template-columns: 1fr;
      min-height: 0;
    }

    .side {
      padding-top: 12px;
    }

    .join-prompt {
      margin-top: 28px;
    }
  }

  @media (max-width: 600px) {
    .stage {
      margin-top: 16px;
    }

    .now {
      flex-direction: column;
      align-items: center;
      padding-top: 10px;
    }

    .cover {
      width: min(72vw, 300px);
    }

    .song-info {
      width: 100%;
      text-align: center;
    }

    .play-here {
      text-align: center;
    }

    .controls {
      justify-content: center;
    }

    .skip-vote {
      margin: 12px auto 0;
    }

    .idle {
      align-items: center;
    }
  }
</style>
