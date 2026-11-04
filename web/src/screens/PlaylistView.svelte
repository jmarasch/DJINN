<script lang="ts">
  import { api } from '../lib/api'
  import Art from '../lib/Art.svelte'
  import { byline, duration } from '../lib/format'
  import Icon from '../lib/Icon.svelte'
  import PlaylistSharing from '../lib/PlaylistSharing.svelte'
  import { app, canStartAutoMix, isAdmin, toast } from '../lib/state.svelte'
  import type { PlaylistCheck, PlaylistDetail, PlaylistEntry, UnplayableSong } from '../lib/types'

  let { id }: { id: number } = $props()
  let playlist = $state<PlaylistDetail | null>(null)
  let renaming = $state(false)
  let name = $state('')

  async function load() {
    playlist = await api.playlist(id)
    name = playlist.name
  }

  $effect(() => {
    void id
    load().catch((e) => toast(e.message, true))
  })

  const playing = $derived(app.player.playlist?.id === id)
  const mine = $derived(playlist?.owner_id === app.me?.id)
  const totalMs = $derived(playlist?.tracks.reduce((sum, t) => sum + (t.duration_ms ?? 0), 0) ?? 0)
  const kindLabel = { track: 'Song', album: 'Album', artist: 'Artist', wanted: 'Song' }
  const notInLibrary = $derived(playlist?.entries.filter((e) => e.kind === 'wanted').length ?? 0)
  const problems = $derived(notInLibrary + (playlist?.unplayable.length ?? 0))

  let check = $state<PlaylistCheck | null>(null)
  const checking = $derived(check?.running && check.playlist_id === id)
  let timer: ReturnType<typeof setTimeout> | undefined

  async function followCheck() {
    check = await api.playlistCheck()
    if (check?.running) {
      timer = setTimeout(() => followCheck().catch(() => {}), 1000)
    } else if (check) {
      await load()
      toast(check.problems ? `${check.problems} of ${check.total} files won't play` : `All ${check.total} files play`)
    }
  }

  async function startCheck() {
    try {
      await api.checkPlaylist(id)
      await followCheck()
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not check the files', true)
    }
  }

  $effect(() => () => clearTimeout(timer))

  function reason(song: UnplayableSong) {
    return song.removed ? 'File is missing' : `File won't play: ${song.error ?? 'unknown error'}`
  }

  /** What's wrong with an entry, in words, if anything. */
  function flagOf(entry: PlaylistEntry) {
    if (entry.kind === 'wanted') return 'Not in your library'
    if (entry.name === null) return 'No longer in your library'
    if (!entry.problem_songs) return null
    if (entry.kind === 'track') {
      const song = playlist?.unplayable.find((s) => s.track_id === entry.ref_id)
      return song ? reason(song) : "Can't play"
    }
    return `${entry.problem_songs} ${entry.problem_songs === 1 ? "song can't" : "songs can't"} play`
  }

  async function attempt(action: () => Promise<unknown>, done?: string) {
    try {
      await action()
      if (done) toast(done)
      await load()
    } catch (e) {
      toast(e instanceof Error ? e.message : 'That did not work', true)
    }
  }

  async function rename(e: SubmitEvent) {
    e.preventDefault()
    renaming = false
    if (name.trim() && name !== playlist?.name) await attempt(() => api.updatePlaylist(id, { name }))
  }

  async function request(trackId: number, title: string) {
    try {
      await api.addRequest(trackId)
      toast(`Requested ${title}`)
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not request it', true)
    }
  }

  function move(entry: PlaylistEntry, index: number) {
    return attempt(() => api.movePlaylistEntry(id, entry.id, index))
  }

  async function remove() {
    if (!playlist || !confirm(`Delete the playlist "${playlist.name}"?`)) return
    try {
      await api.deletePlaylist(id)
      location.hash = '/playlists'
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not delete it', true)
    }
  }

  const hours = (ms: number) => {
    const minutes = Math.round(ms / 60000)
    return minutes < 60 ? `${minutes} min` : `${Math.floor(minutes / 60)} h ${minutes % 60} min`
  }
</script>

<button class="back" onclick={() => history.back()}><Icon name="back" /> Back</button>

{#if playlist}
  {#if renaming}
    <form class="rename" onsubmit={rename}>
      <!-- svelte-ignore a11y_autofocus -->
      <input bind:value={name} maxlength="60" autofocus />
      <button class="primary">Save</button>
    </form>
  {:else}
    <h1>
      {#if mine}
        <button class="name" onclick={() => (renaming = true)} title="Rename">{playlist.name}</button>
      {:else}
        {playlist.name}
      {/if}
    </h1>
  {/if}
  <p class="muted small">
    {playlist.tracks.length} {playlist.tracks.length === 1 ? 'song' : 'songs'} · {hours(totalMs)}
    {#if !mine}· from {playlist.owner}{/if}
    {#if playing}· <b class="accent">playing now</b>{/if}
    {#if problems}· <span class="bad"><Icon name="warning" size={13} /> {problems} can't play and will be skipped</span>{/if}
  </p>

  {#if canStartAutoMix() || mine}
    <div class="actions">
      {#if canStartAutoMix()}
        <button class="primary" onclick={() => attempt(() => api.playPlaylist(id), `Playing ${playlist?.name}`)}>
          <Icon name="play" size={20} /> Play
        </button>
      {/if}
      {#if mine}
        <div class="mode" role="group" aria-label="Play order">
          <button
            class:active={playlist.shuffle}
            aria-pressed={playlist.shuffle}
            onclick={() => attempt(() => api.updatePlaylist(id, { shuffle: true }))}
          >
            <Icon name="shuffle" size={18} /> Shuffle
          </button>
          <button
            class:active={!playlist.shuffle}
            aria-pressed={!playlist.shuffle}
            onclick={() => attempt(() => api.updatePlaylist(id, { shuffle: false }))}
          >
            <Icon name="ordered" size={18} /> In order
          </button>
        </div>
      {/if}
    </div>
  {/if}
  {#if playing && canStartAutoMix()}
    <button class="stop" onclick={() => attempt(() => api.stopPlaylist(), 'Stopped')}>Stop playing this playlist</button>
  {/if}

  {#if isAdmin()}
    <button class="stop" disabled={checking} onclick={startCheck}>
      {checking ? `Checking files: ${check?.done ?? 0} of ${check?.total ?? 0}` : 'Check files'}
    </button>
  {/if}

  {#if mine}
    <PlaylistSharing
      {id}
      shared={playlist.shared}
      setShared={(shared) => attempt(() => api.updatePlaylist(id, { shared }))}
    />
  {/if}

  <h2>What's in it</h2>
  {#each playlist.entries as entry, i (entry.id)}
    {@const flag = flagOf(entry)}
    <div class="entry" class:flagged={flag && (entry.kind === 'track' || entry.kind === 'wanted')}>
      <Art hash={entry.art_hash} size={40} round={entry.kind === 'artist' ? 999 : 6} />
      <div class="text">
        <div class="ellipsis">
          {#if entry.kind === 'album'}
            <a href="#/album/{entry.ref_id}">{entry.name ?? '(missing)'}</a>
          {:else if entry.kind === 'artist'}
            <a href="#/artist/{entry.ref_id}">{entry.name ?? '(missing)'}</a>
          {:else}
            {entry.name ?? '(missing)'}
          {/if}
        </div>
        <div class="muted small ellipsis">{kindLabel[entry.kind]}{entry.subtitle ? ` · ${entry.subtitle}` : ''}</div>
        {#if flag}
          <div class="bad small ellipsis" title={flag}><Icon name="warning" size={13} /> {flag}</div>
        {/if}
      </div>
      {#if mine}
      <div class="order">
        <button aria-label="Move up" disabled={i === 0} onclick={() => move(entry, i - 1)}><Icon name="up" size={18} /></button>
        <button aria-label="Move down" disabled={i === playlist.entries.length - 1} onclick={() => move(entry, i + 1)}
          ><Icon name="down" size={18} /></button
        >
      </div>
      <button class="icon" aria-label="Remove {entry.name}" onclick={() => attempt(() => api.removeFromPlaylist(id, entry.id))}>
        <Icon name="close" />
      </button>
      {/if}
    </div>
  {:else}
    <p class="muted">Empty. Open an album, artist, or song in the Library and tap the playlist button to add it here.</p>
  {/each}

  {#if playlist.tracks.length}
    <h2>Songs</h2>
    <p class="muted small">
      {canStartAutoMix() ? 'Tap a song to play the playlist from there.' : 'Tap a song to request it.'}
    </p>
    {#each playlist.tracks as track (track.id)}
      <div class="song-row">
        <button
          class="song"
          onclick={() =>
            canStartAutoMix()
              ? attempt(() => api.playPlaylist(id, track.id), `Playing from ${track.title}`)
              : request(track.id, track.title)}
        >
          <Art hash={track.art_hash} size={40} />
          <span class="text">
            <span class="ellipsis block">{track.title}</span>
            <span class="muted small ellipsis block">{byline(track.artist, track.album)}</span>
          </span>
          <span class="muted small">{duration(track.duration_ms)}</span>
        </button>
        {#if canStartAutoMix()}
          <button class="icon" aria-label="Request {track.title}" title="Request" onclick={() => request(track.id, track.title)}>
            <Icon name="queue" />
          </button>
        {/if}
      </div>
    {/each}
  {/if}

  {#if playlist.unplayable.length}
    <h2>Can't play</h2>
    <p class="muted small">These are skipped. A library check tries them again.</p>
    {#each playlist.unplayable as song (song.track_id)}
      <div class="entry flagged">
        <div class="text">
          <div class="ellipsis">{song.title}</div>
          <div class="muted small ellipsis">{byline(song.artist, song.album)}</div>
          <div class="bad small ellipsis" title={reason(song)}><Icon name="warning" size={13} /> {reason(song)}</div>
        </div>
      </div>
    {/each}
  {/if}

  {#if mine}
    <button class="delete" onclick={remove}>Delete playlist</button>
  {/if}
{/if}

<style>
  h1 {
    margin: 4px 0 2px;
  }

  .name {
    min-height: 0;
    padding: 0;
    background: transparent;
    color: var(--text);
    font: inherit;
    text-align: left;
  }

  .rename {
    display: flex;
    gap: 8px;
    margin: 4px 0;
  }

  .rename input {
    flex: 1;
    min-width: 0;
  }

  .accent {
    color: var(--accent);
  }

  .bad {
    color: var(--bad);
  }

  .flagged .text > div:first-child {
    color: var(--muted);
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 10px;
    margin: 12px 0 4px;
  }

  .mode {
    display: flex;
    gap: 4px;
    padding: 4px;
    border-radius: 12px;
    background: var(--surface);
  }

  .mode button {
    min-height: 36px;
    padding: 0 10px;
    background: transparent;
    color: var(--muted);
    font-size: 0.875rem;
  }

  .mode button.active {
    background: var(--surface-3);
    color: var(--text);
    font-weight: 600;
  }

  .stop {
    margin-top: 6px;
  }

  .entry,
  .song {
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: 60px;
    border-bottom: 1px solid var(--line);
  }

  .song-row {
    display: flex;
    align-items: center;
  }

  .song {
    flex: 1;
    min-width: 0;
    width: 100%;
    justify-content: flex-start;
    padding: 0;
    border-radius: 0;
    background: transparent;
    text-align: left;
  }

  .text {
    flex: 1;
    min-width: 0;
  }

  .entry a {
    color: var(--text);
  }

  .block {
    display: block;
  }

  .order {
    display: flex;
    flex-direction: column;
  }

  .order button {
    width: 36px;
    min-height: 26px;
    padding: 0;
    background: transparent;
    color: var(--muted);
  }

  .icon {
    width: 40px;
    padding: 0;
    background: transparent;
    color: var(--muted);
  }

  .delete {
    width: 100%;
    margin: 24px 0;
    color: var(--bad);
  }
</style>
