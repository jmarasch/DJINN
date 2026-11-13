<script lang="ts">
  import MoodDial from '../lib/MoodDial.svelte'
  import { t } from '../lib/i18n.svelte'
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
  import Heart from '../lib/Heart.svelte'
  import LyricsView from '../lib/LyricsView.svelte'
  import type { Lyrics } from '../lib/types'

  const current = $derived(app.nowPlaying.current)
  const next = $derived(app.nowPlaying.up_next)
  /** Auto mix songs after the requests, in the room the requests leave. */
  const SHOWN = 6
  const autoNext = $derived(
    [
      ...(next && next.source !== 'request' && next.track.id !== current?.track.id ? [next] : []),
      ...(app.nowPlaying.later ?? []),
    ].slice(0, Math.max(0, SHOWN - app.queue.length)),
  )
  const total = $derived(current?.track.duration_ms ?? 0)
  let position = $state(0)
  let volume = $state(app.player.volume)

  // Lyrics under the cover whenever the song has some.
  let lyrics = $state<Lyrics | null | undefined>(undefined)
  const hasLyrics = $derived(!!lyrics && !lyrics.instrumental && !!(lyrics.lines?.length || lyrics.plain))
  $effect(() => {
    const id = current?.track.id
    lyrics = undefined
    if (id == null) return
    let live = true
    api
      .trackLyrics(id)
      .then((found) => {
        if (live) lyrics = found
      })
      .catch(() => {
        if (live) lyrics = null
      })
    return () => {
      live = false
    }
  })

  // How many people have the app open in this room.
  let people = $state(0)
  async function loadPeople() {
    try {
      people = (await api.roomPeople()).count
    } catch {
      // The count is a nicety; leave it as it was.
    }
  }

  onMount(() => {
    void checkPlayHere()
    void loadPeople()
    // Again soon: this page's own connection isn't open at the first look.
    const soon = setTimeout(() => void loadPeople(), 1500)
    const counting = setInterval(() => void loadPeople(), 15_000)
    let frame = 0
    const tick = () => {
      position = Math.min(currentPosition(), total || Infinity)
      frame = requestAnimationFrame(tick)
    }
    tick()
    return () => {
      cancelAnimationFrame(frame)
      clearInterval(counting)
      clearTimeout(soon)
    }
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
      toast(skipped ? t('playing.skipped') : t('playing.vote_counted'))
    } catch (e) {
      toast(e instanceof Error ? e.message : t('playing.vote_failed'), true)
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
    {#if people > 0}
      <span class="room-pill people" title={t('playing.people_here', { n: people })}>
        <Icon name="person" size={14} />
        {people}
        <span class="bars" aria-hidden="true"><i></i><i></i><i></i></span>
      </span>
    {/if}
    {#if app.rooms.length > 1}
      <span class="room-pill"><Icon name="home" size={14} /> {app.rooms.find((r) => r.id === app.room)?.name ?? ''}</span>
    {/if}
    <a class="button" href={app.room > 1 ? `/tv?room=${app.room}` : '/tv'} title="Switch to TV display">TV display</a>
    <a
      class="button"
      href={app.room > 1 ? `/visualizer?room=${app.room}` : '/visualizer'}
      title="A visualizer in step with this room's speakers, with the song and what's coming up">Visualizer</a
    >
  </div>
</div>

{#snippet wave(side: string)}
  <svg class="wave {side}" width="56" height="60" viewBox="0 0 56 60" aria-hidden="true">
    {#each [8, 18, 30, 44, 26, 38, 16, 10] as h, i (i)}
      <rect x={side === 'left' ? i * 7 : 49 - i * 7} y={30 - h / 2} width="3" height={h} rx="1.5" />
    {/each}
  </svg>
{/snippet}

<div class="stage">
<section class="now">
  {#if current}
    <div class="left">
      <div class="cover">
        <span class="disc" aria-hidden="true"></span>
        {@render wave('left')}
        <Art hash={current.track.art_hash} size={420} round={16} />
        {@render wave('right')}
      </div>
      {#if hasLyrics}
        <div class="lyrics-card"><LyricsView {lyrics} position={currentPosition} /></div>
      {/if}
    </div>
  {/if}
  <div class="song-info">
  {#if current}
    <span class="label">{app.player.state === 'paused' ? t('playing.paused') : t('playing.now_playing')}</span>
    <h1 class="title">{current.track.title}</h1>
    <p class="artist">
      {#if current.track.credits.length || current.track.artist}
        <Credits track={current.track} />
      {:else}
        {t('common.unknown_artist')}
      {/if}
      {#if current.track.album_id}
        · <a href="#/album/{current.track.album_id}">{current.track.album}</a>
      {/if}
    </p>
    {#if requesterText}
      <p class="badge"><Icon name="person" size={18} /> {t('playing.requested_by', { name: requesterText })}</p>
    {/if}
    <div class="progress" role="progressbar" aria-valuenow={position} aria-valuemax={total}>
      <div class="fill" style:width="{total ? (position / total) * 100 : 0}%"></div>
    </div>
    <div class="times">
      <span>{duration(position)}</span>
      <span>{duration(total)}</span>
    </div>
    {#if canVote}
    <div class="actions">
      <button class="skip-vote" disabled={voted || resting} onclick={voteSkip}>
        <Icon name="skip" size={18} />
        {#if soleRequester}
          {t('playing.skip_mine')}
        {:else if resting}
          {t('playing.skip_resting')}
        {:else if voted}
          {t('playing.voted', { votes: app.player.skip_votes, needed: app.player.skip_votes_needed })}
        {:else}
          {t('playing.vote_skip')}{app.player.skip_votes
            ? ` · ${t('playing.votes_of', { votes: app.player.skip_votes, needed: app.player.skip_votes_needed })}`
            : ''}
        {/if}
      </button>
    </div>
    {/if}
  {:else}
    <div class="idle">
      <img src="/images/nothing-playing.webp" alt="" />
      <h1>{t('playing.nothing')}</h1>
      <p class="muted">
        {app.output.error ? t('playing.output_problem', { error: app.output.error }) : t('playing.nothing_hint')}
      </p>
      <a class="button primary" href="#/search">Find a song</a>
    </div>
  {/if}

  <MoodDial />

  {#if playHere.url}
    <div class="play-here">
      <button
        class="tune neon"
        class:on={playHere.on}
        class:waiting={playHere.on && playHere.waiting}
        aria-pressed={playHere.on}
        onclick={() => (playHere.on ? stopPlayHere() : startPlayHere())}
      >
        <Icon name="headphones" size={30} />
        <span class="tune-text">
          <strong>{t('playing.tune_in')}</strong>
          <small>
            {#if playHere.on}
              {playHere.waiting ? t('playing.tune_in_connecting') : t('playing.tune_in_on')}
            {:else}
              {t('playing.tune_in_hint')}
            {/if}
          </small>
        </span>
        <svg class="tune-wave" width="70" height="28" viewBox="0 0 70 28" aria-hidden="true">
          {#each [6, 12, 20, 10, 24, 14, 8, 18, 12, 6, 10, 4] as h, i (i)}
            <rect x={i * 6} y={14 - h / 2} width="2.4" height={h} rx="1.2" />
          {/each}
        </svg>
      </button>
    </div>
  {/if}

  {#if isAdmin()}
    <div class="admin-controls">
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
      <button
        class="round"
        aria-label="Start over"
        title="Start this song again from the beginning"
        disabled={!current}
        onclick={() => control(api.startOver)}
      >
        <Icon name="previous" size={26} />
      </button>
      <button class="round" aria-label="Skip" title="Skip" disabled={!current} onclick={() => control(api.skip)}>
        <Icon name="skip" size={26} />
      </button>
      <button
        class="round dislike"
        aria-label="Skip and play less like this"
        title="Skip it, and the auto mix plays it (and songs like it) less"
        disabled={!current}
        onclick={() => control(api.skipLess)}
      >
        <Icon name="thumb-down" size={24} />
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
    </div>
  {/if}
  </div>
</section>

  <aside class="side neon">
    <h2>{t('playing.requests')}</h2>
    {#if app.queue.length}
      <ol class="queue-list">
        {#each app.queue.slice(0, SHOWN) as item, i (item.id)}
          {@const mine = myRequest(item.track.id)}
          <li>
            <span class="num">{i + 1}</span>
            <Art hash={item.track.art_hash} size={56} />
            <div class="text">
              <div class="ellipsis">{item.track.title}</div>
              <div class="muted small ellipsis">{byline(item.track.artist)}</div>
              <div class="small requester">{askedBy(item)}</div>
            </div>
            {#if mine}
              <button class="cancel" onclick={() => cancelRequest(mine.id)}>
                {involvement(mine) === 'backer' ? t('playing.step_back') : t('common.cancel')}
              </button>
            {/if}
          </li>
        {/each}
      </ol>
      {#if app.queue.length > SHOWN}<p class="muted small">{t('playing.and_more', { n: app.queue.length - SHOWN })}</p>{/if}
    {:else}
      <p class="muted small">{t('playing.no_requests')}</p>
    {/if}

    {#if autoNext.length}
      <h2 class="coming">{t('playing.coming_up')}</h2>
      {#each autoNext as entry, i (`${entry.track.id}-${i}`)}
        <div class="next-row">
          <Art hash={entry.track.art_hash} size={40} />
          <div class="text">
            <div class="ellipsis">{entry.track.title}</div>
            <div class="muted small ellipsis">{byline(entry.track.artist)}</div>
          </div>
          <Heart kind="track" id={entry.track.id} name={entry.track.title} />
        </div>
      {/each}
    {/if}
  </aside>
</div>

  {#if !isAdmin()}
    <p class="host-link">
      {#if app.me?.role === 'guest'}
        {t('playing.in_as', { name: app.me.display_name ?? '' })}
        <button onclick={signInAsHost}>{t('join.host_sign_in')}</button>
      {:else}
        {t('playing.signed_in_as', { name: app.me?.username ?? '' })} <a href="#/account">{t('playing.your_account')}</a>
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

  .tune {
    display: flex;
    width: 100%;
    justify-content: flex-start;
    gap: 14px;
    min-height: 72px;
    padding: 12px 18px;
    border-radius: 16px;
    color: var(--accent);
    text-align: left;
  }

  .tune-text {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-width: 0;
  }

  .tune-text strong {
    color: var(--text);
    font-size: 1.15rem;
  }

  .tune-text small {
    color: var(--muted);
    font-size: 0.875rem;
  }

  .tune-wave {
    flex: none;
    fill: var(--accent);
    opacity: 0.55;
  }

  .play-here .tune.on {
    border-color: var(--edge-strong);
    box-shadow: var(--glow-strong);
  }

  .play-here .tune.waiting {
    animation: tuning 1.2s ease-in-out infinite alternate;
  }

  @keyframes tuning {
    to {
      box-shadow: 0 0 4px var(--accent);
      opacity: 0.75;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .play-here .tune.waiting {
      animation: none;
    }
  }



  .skip-vote {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    min-height: 52px;
    padding: 0 18px;
    border: 1px solid var(--edge-strong);
    border-radius: 14px;
    background: var(--accent);
    color: var(--accent-text);
    font-size: 1.05rem;
    font-weight: 700;
    box-shadow: var(--glow-strong);
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

  .room-pill {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    min-height: 34px;
    padding: 0 12px;
    border: 1px solid var(--edge);
    border-radius: 999px;
    background: var(--surface);
    box-shadow: var(--glow);
    color: var(--text);
    font-size: 0.8125rem;
    font-weight: 600;
  }

  /* Three small bars that move, so the room feels alive. */
  .bars {
    display: inline-flex;
    align-items: flex-end;
    gap: 2px;
    height: 12px;
  }

  .bars i {
    width: 2px;
    height: 100%;
    border-radius: 1px;
    background: var(--accent-2);
    transform-origin: bottom;
    animation: bounce 1.1s ease-in-out infinite;
  }

  .bars i:nth-child(2) {
    animation-delay: 0.25s;
  }

  .bars i:nth-child(3) {
    animation-delay: 0.5s;
  }

  @keyframes bounce {
    0%,
    100% {
      transform: scaleY(0.3);
    }
    50% {
      transform: scaleY(1);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .bars i {
      animation: none;
    }
  }

  .left {
    flex: none;
    display: flex;
    flex-direction: column;
    gap: 14px;
    width: min(28vw, 330px);
    margin-right: min(4vw, 44px);
  }

  .lyrics-card {
    max-height: 360px;
    overflow: hidden;
    padding: 12px;
    border: 1px solid var(--edge);
    border-radius: 14px;
    background: var(--card-bg);
    box-shadow: var(--glow);
  }

  /* The host's buttons and volume, in a neon card. */
  .admin-controls {
    margin-top: 14px;
    padding: 4px 16px 14px;
    border: 1px solid var(--edge);
    border-radius: 16px;
    background: var(--card-bg);
    box-shadow: var(--glow);
  }

  .dislike {
    color: var(--muted);
  }

  .dislike:hover:not(:disabled) {
    color: var(--bad);
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


  .actions {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 12px;
    margin: 18px 0 8px;
  }

  .actions > :only-child {
    grid-column: 1 / -1;
    justify-self: start;
  }

  /* A record peeking out behind the cover, as on a turntable. */
  .disc {
    position: absolute;
    inset: 0 -12% 0 12%;
    border-radius: 50%;
    background:
      radial-gradient(circle, #2a2140 0 9%, #0d0a14 9.5% 12%, transparent 12.5%),
      repeating-radial-gradient(circle, #15111f 0 2px, #1d1729 2px 4px);
    box-shadow:
      0 0 0 1px rgb(180 140 255 / 0.25),
      0 0 30px rgb(180 140 255 / 0.15);
  }

  .wave {
    display: none;
    position: absolute;
    top: 50%;
    fill: var(--accent);
    filter: drop-shadow(0 0 5px rgb(180 140 255 / 0.7));
    transform: translateY(-50%);
  }

  .wave.left {
    right: calc(100% + 8px);
  }

  .wave.right {
    left: calc(100% + 26px);
  }

  .num {
    flex: none;
    width: 18px;
    color: var(--muted);
    font-weight: 600;
    text-align: center;
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
    position: relative;
    width: 100%;
    aspect-ratio: 1;
  }

  .cover :global(.art) {
    position: relative;
    z-index: 1;
    width: 100% !important;
    height: 100% !important;
    border: 1px solid var(--edge-strong);
    box-shadow:
      0 16px 48px rgb(0 0 0 / 0.45),
      0 0 24px rgb(180 140 255 / 0.3);
  }

  .title {
    margin: 8px 0 10px;
    font-size: clamp(1.75rem, 3vw, 3rem);
    line-height: 1.1;
    overflow-wrap: anywhere;
  }

  .artist {
    margin: 0;
    color: var(--accent);
    font-size: 1.25rem;
  }

  .artist a {
    color: inherit;
  }

  .badge {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    margin: 16px 0 0;
    padding: 6px 16px;
    border: 1px solid var(--edge);
    border-radius: 12px;
    background: var(--surface-2);
    color: var(--muted);
    font-size: 0.95rem;
  }

  .progress {
    width: 100%;
    height: 8px;
    margin-top: 22px;
    border-radius: 4px;
    background: var(--surface-3);
    overflow: hidden;
  }

  .fill {
    height: 100%;
    border-radius: 4px;
    background: linear-gradient(90deg, #9b6dff, var(--accent));
    box-shadow: 0 0 12px rgb(180 140 255 / 0.7);
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
    align-self: start;
    min-width: 0;
    margin-top: 26px;
    padding: 20px;
    border-radius: 18px;
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
    gap: 10px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .queue-list li {
    display: flex;
    align-items: center;
    gap: 12px;
    min-width: 0;
    padding: 10px 12px;
    border: 1px solid var(--edge);
    border-radius: 14px;
    background: rgb(255 255 255 / 0.02);
  }

  .requester {
    color: var(--accent);
  }





  .label {
    display: block;
    color: var(--accent);
    font-size: 0.8rem;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.3em;
  }

  .coming {
    margin-top: 16px;
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

    .label {
      display: flex;
      align-items: center;
      gap: 12px;
      margin-top: 18px;
    }

    .label::before,
    .label::after {
      content: '';
      flex: 1;
      height: 1px;
      background: linear-gradient(90deg, transparent, var(--accent));
    }

    .label::after {
      background: linear-gradient(90deg, var(--accent), transparent);
    }

    .left {
      width: min(62vw, 280px);
      margin-right: 0;
    }

    .disc {
      inset: -10%;
    }

    .wave {
      display: block;
    }

    .idle {
      align-items: center;
    }
  }
</style>
