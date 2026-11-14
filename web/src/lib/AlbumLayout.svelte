<script lang="ts">
  // The library editor's album layout, like Picard: pick a release of the
  // album (every one MusicBrainz has, or search MusicBrainz, Deezer or
  // Apple by hand when it found the wrong album or artist), see its track
  // list with the songs laid on it (matched by title), drag songs that
  // didn't land, or landed wrong, onto the right track from Unknown, then
  // take the release's titles, numbers, album, artists, year and cover.
  // The tags go back to the grid as changes to approve.
  import { onMount } from 'svelte'
  import { api } from './api'
  import { unlocked } from './protected.svelte'
  import { toast } from './state.svelte'
  import type { EditorField, EditorSong, Release, ReleaseHit, ReleaseSource } from './types'

  let {
    songs,
    onapply,
    onclose,
  }: {
    songs: EditorSong[]
    /** The tags to change, and the release's cover if it is to be used. */
    onapply: (changes: { song: EditorSong; field: EditorField; value: string }[], cover: Blob | null) => void
    onclose: () => void
  } = $props()

  const SOURCES: { key: ReleaseSource; label: string }[] = [
    { key: 'musicbrainz', label: 'MusicBrainz' },
    { key: 'deezer', label: 'Deezer' },
    { key: 'itunes', label: 'Apple Music (iTunes)' },
  ]

  // svelte-ignore state_referenced_locally
  const first = songs[0]
  let source = $state<ReleaseSource>('musicbrainz')
  let artist = $state(first?.album_artist || first?.artist || '')
  let album = $state(first?.album || '')
  let hits = $state<ReleaseHit[]>([])
  let hitsAbout = $state('')
  let searching = $state(false)
  let release = $state<Release | null>(null)
  let chosenHit = $state<ReleaseHit | null>(null)
  let loadingRelease = $state(false)
  /** The song on each track of the release, by track index. */
  let laid = $state<(number | null)[]>([])
  /** A song picked up by a tap, to put on a track with another tap. */
  let carrying = $state<number | null>(null)
  let take = $state({ titles: true, numbers: true, album: true, album_artist: true, artist: false, year: true, cover: false })

  const byId = (id: number | null) => (id === null ? undefined : songs.find((s) => s.id === id))
  const unknown = $derived(songs.filter((s) => !laid.includes(s.id)))
  const discs = $derived(new Set(release?.tracks.map((t) => t.disc) ?? []).size)

  function length(ms: number | null | undefined): string {
    if (!ms) return ''
    const s = Math.round(ms / 1000)
    return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`
  }

  const fileName = (path: string) => path.slice(Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\')) + 1)
  /** The song's folder inside its library folder. */
  const folderOf = (song: EditorSong) => {
    const rel = song.rel_path || song.path
    return rel.slice(0, Math.max(rel.lastIndexOf('/'), rel.lastIndexOf('\\')))
  }

  onMount(async () => {
    const albumId = first?.album_id
    if (albumId == null) return
    searching = true
    try {
      const found = await unlocked(() => api.editorAlbumReleases(albumId))
      if (found.known) {
        hits = found.releases
        hitsAbout = 'Releases of this album on MusicBrainz'
        // Like Picard: start from the release closest in track count.
        const best = [...hits].sort(
          (a, b) => Math.abs(a.tracks - songs.length) - Math.abs(b.tracks - songs.length),
        )[0]
        if (best) await choose(best)
      } else {
        hitsAbout = "MusicBrainz doesn't know this album yet: search for it below."
      }
    } catch (e) {
      hitsAbout = e instanceof Error ? e.message : 'Could not ask MusicBrainz'
    } finally {
      searching = false
    }
  })

  async function search(e: SubmitEvent) {
    e.preventDefault()
    searching = true
    try {
      hits = await unlocked(() => api.editorReleaseSearch(source, artist, album))
      hitsAbout = `${hits.length} found on ${SOURCES.find((s) => s.key === source)?.label}`
    } catch (err) {
      hits = []
      hitsAbout = err instanceof Error ? err.message : 'The search failed'
    } finally {
      searching = false
    }
  }

  async function choose(hit: ReleaseHit) {
    chosenHit = hit
    loadingRelease = true
    try {
      const found = await unlocked(() => api.editorRelease(hit.source, hit.id, songs.map((s) => s.id)))
      release = found.release
      const next: (number | null)[] = found.release.tracks.map(() => null)
      for (const m of found.matched) next[m.track] = m.id
      laid = next
      carrying = null
      take.artist = found.release.tracks.some((t) => t.artist)
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not get its tracks', true)
    } finally {
      loadingRelease = false
    }
  }

  /** Puts a song on a track (the one there goes to Unknown); null takes it off. */
  function place(track: number | null, id: number) {
    const next = laid.map((x) => (x === id ? null : x))
    if (track !== null) next[track] = id
    laid = next
    carrying = null
  }

  function drop(e: DragEvent, track: number | null) {
    e.preventDefault()
    const id = Number(e.dataTransfer?.getData('text/plain'))
    if (id) place(track, id)
  }

  function drag(e: DragEvent, id: number) {
    e.dataTransfer?.setData('text/plain', String(id))
    if (e.dataTransfer) e.dataTransfer.effectAllowed = 'move'
  }

  async function apply() {
    if (!release) return
    const changes: { song: EditorSong; field: EditorField; value: string }[] = []
    release.tracks.forEach((track, i) => {
      const song = byId(laid[i])
      if (!song || !release) return
      const set = (field: EditorField, value: string | number | null | undefined) => {
        if (value !== null && value !== undefined && value !== '') changes.push({ song, field, value: String(value) })
      }
      if (take.titles) set('title', track.title)
      if (take.numbers) {
        set('track_no', track.number)
        if (discs > 1 || song.disc_no) set('disc_no', track.disc)
      }
      if (take.album) set('album', release.title)
      if (take.album_artist) set('album_artist', release.artist)
      if (take.artist) set('artist', track.artist ?? release.artist)
      if (take.year) set('year', release.year)
    })
    let cover: Blob | null = null
    if (take.cover && chosenHit) {
      const res = await fetch(api.editorReleaseCover(chosenHit.source, chosenHit.id))
      if (res.ok) cover = await res.blob()
      else toast("This release has no cover to take; the tags are still taken", true)
    }
    onapply(changes, cover)
  }
</script>

<section class="layout">
  <header>
    <strong>Album layout</strong>
    <span class="small muted">{songs.length} songs{first?.album ? ` from ${first.album}` : ''}</span>
    <button class="small-button close" onclick={onclose}>Close</button>
  </header>

  <div class="cols">
    <div class="find">
      <form onsubmit={search}>
        <select bind:value={source} aria-label="Where to look">
          {#each SOURCES as s (s.key)}<option value={s.key}>{s.label}</option>{/each}
        </select>
        <input bind:value={artist} placeholder="Artist" aria-label="Artist" />
        <input bind:value={album} placeholder="Album" aria-label="Album" />
        <button class="small-button" type="submit" disabled={searching}>Search</button>
      </form>
      <p class="small muted">{searching ? 'Looking...' : hitsAbout}</p>
      <ul class="hits">
        {#each hits as hit (hit.source + hit.id)}
          <li>
            <button class="hit" class:on={chosenHit?.id === hit.id} onclick={() => choose(hit)}>
              <b>{hit.title}</b>
              <span class="small">{hit.artist}</span>
              <span class="small muted">
                {[hit.date, hit.country, hit.format, `${hit.tracks} tracks`, hit.note].filter(Boolean).join(' · ')}
              </span>
            </button>
          </li>
        {/each}
      </ul>
    </div>

    <div class="tracks">
      {#if loadingRelease}
        <p class="small muted">Getting the track list...</p>
      {:else if release}
        <p class="small">
          <b>{release.title}</b> by {release.artist}{release.year ? ` (${release.year})` : ''} · {release.tracks.length} tracks ·
          {laid.filter((x) => x !== null).length} of {songs.length} songs placed
        </p>
        <p class="small muted">Drag a song onto its track (or tap it, then tap the track). Dropping it on Unknown takes it off.</p>
        <table>
          <tbody>
            {#each release.tracks as track, i (i)}
              {#if discs > 1 && (i === 0 || release.tracks[i - 1].disc !== track.disc)}
                <tr class="disc"><td colspan="3">Disc {track.disc}</td></tr>
              {/if}
              <tr>
                <td class="num">{track.number}</td>
                <td class="title">
                  {track.title}{#if track.artist}<span class="small muted"> · {track.artist}</span>{/if}
                  <span class="small muted"> {length(track.length_ms)}</span>
                </td>
                <td
                  class="slot"
                  class:empty={laid[i] === null}
                  class:target={carrying !== null}
                  ondragover={(e) => e.preventDefault()}
                  ondrop={(e) => drop(e, i)}
                  onclick={() => carrying !== null && place(i, carrying)}
                  onkeydown={(e) => e.key === 'Enter' && carrying !== null && place(i, carrying)}
                  role="button"
                  tabindex="-1"
                >
                  {#if byId(laid[i])}
                    {@const song = byId(laid[i])!}
                    <div
                      class="song"
                      class:carried={carrying === song.id}
                      draggable="true"
                      role="button"
                      tabindex="0"
                      ondragstart={(e) => drag(e, song.id)}
                      onclick={(e) => {
                        e.stopPropagation()
                        carrying = carrying === song.id ? null : song.id
                      }}
                      onkeydown={(e) => e.key === 'Enter' && (carrying = song.id)}
                      title={song.rel_path}
                    >
                      <span class="mono">{fileName(song.path)}</span>
                      <span class="small muted">{length(song.duration_ms)}{song.title !== track.title ? ` · now "${song.title}"` : ''}</span>
                      <span class="small muted mono path">{folderOf(song)}</span>
                    </div>
                  {:else}
                    <span class="small muted">{carrying !== null ? 'Put it here' : 'Drop a song here'}</span>
                  {/if}
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      {:else}
        <p class="small muted">Pick a release on the left to lay the songs on it.</p>
      {/if}

      <div
        class="unknown"
        class:target={carrying !== null}
        ondragover={(e) => e.preventDefault()}
        ondrop={(e) => drop(e, null)}
        onclick={() => carrying !== null && place(null, carrying)}
        onkeydown={(e) => e.key === 'Enter' && carrying !== null && place(null, carrying)}
        role="button"
        tabindex="-1"
      >
        <div class="small"><b>Unknown</b> <span class="muted">({unknown.length}): songs not on a track</span></div>
        {#each unknown as song (song.id)}
          <div
            class="song"
            class:carried={carrying === song.id}
            draggable="true"
            role="button"
            tabindex="0"
            ondragstart={(e) => drag(e, song.id)}
            onclick={(e) => {
              e.stopPropagation()
              carrying = carrying === song.id ? null : song.id
            }}
            onkeydown={(e) => e.key === 'Enter' && (carrying = song.id)}
            title={song.rel_path}
          >
            <span class="mono">{fileName(song.path)}</span>
            <span class="small muted">"{song.title}" {length(song.duration_ms)}</span>
            <span class="small muted mono path">{folderOf(song)}</span>
          </div>
        {/each}
      </div>
    </div>

    <div class="use">
      {#if chosenHit}
        <img src={api.editorReleaseCover(chosenHit.source, chosenHit.id)} alt="The release's cover" width="200" height="200" />
      {/if}
      <div class="small"><b>Take from the release</b></div>
      <label class="check small"><input type="checkbox" bind:checked={take.titles} /> Titles</label>
      <label class="check small"><input type="checkbox" bind:checked={take.numbers} /> Track and disc numbers</label>
      <label class="check small"><input type="checkbox" bind:checked={take.album} /> Album</label>
      <label class="check small"><input type="checkbox" bind:checked={take.album_artist} /> Album artist</label>
      <label class="check small"><input type="checkbox" bind:checked={take.artist} /> Each song's artist</label>
      <label class="check small"><input type="checkbox" bind:checked={take.year} /> Year</label>
      <label class="check small"><input type="checkbox" bind:checked={take.cover} /> Cover</label>
      <button class="primary" disabled={!release || laid.every((x) => x === null)} onclick={apply}>Use this layout</button>
      <p class="small muted">The tags go to the grid as changes to approve. The cover is put on straight away (it can be undone).</p>
    </div>
  </div>
</section>

<style>
  .layout {
    margin-top: 12px;
    border: 1px solid var(--target, var(--accent));
    border-radius: 10px;
    padding: 10px;
  }

  header {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-bottom: 8px;
  }

  .close {
    margin-left: auto;
  }

  .cols {
    display: grid;
    grid-template-columns: 260px minmax(0, 1fr) 220px;
    gap: 12px;
  }

  /* Inside Admin's menu: the release list and the tracks side by side,
     what to take in a row under them. */
  @container (max-width: 1150px) {
    .cols {
      grid-template-columns: 230px minmax(0, 1fr);
    }

    .use {
      grid-column: 1 / -1;
      flex-direction: row !important;
      flex-wrap: wrap;
      align-items: center !important;
    }

    .use img {
      width: 96px !important;
      height: 96px !important;
    }
  }

  @container (max-width: 640px) {
    .cols {
      grid-template-columns: minmax(0, 1fr);
    }
  }

  /* The page's big touch controls are too tall here. */
  .layout input,
  .layout select,
  .layout button:not(.primary) {
    min-height: 0;
  }

  .layout input,
  .layout select {
    padding: 4px 8px;
  }

  .find form {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .hits {
    list-style: none;
    margin: 6px 0;
    padding: 0;
    max-height: 50vh;
    overflow: auto;
  }

  .hit {
    width: 100%;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    text-align: left;
    gap: 0;
    padding: 4px 8px;
    margin-bottom: 4px;
    border-radius: 6px;
    line-height: 1.25;
  }

  .hit.on {
    outline: 2px solid var(--target, var(--accent));
  }

  table {
    width: 100%;
    border-collapse: collapse;
    font-size: 0.88rem;
  }

  td {
    padding: 3px 6px;
    border-bottom: 1px solid var(--line);
    vertical-align: top;
  }

  .num {
    width: 3ch;
    text-align: right;
  }

  .title {
    width: 40%;
  }

  tr.disc td {
    font-weight: 700;
    border-bottom: 1px solid var(--target, var(--accent));
  }

  .slot.empty {
    background: color-mix(in srgb, var(--target, var(--accent)) 8%, transparent);
  }

  .slot.target,
  .unknown.target {
    outline: 1px dashed var(--target, var(--accent));
    cursor: pointer;
  }

  .song {
    display: flex;
    flex-direction: column;
    cursor: grab;
    padding: 2px 4px;
    border-radius: 4px;
  }

  .song.carried {
    outline: 2px solid var(--target, var(--accent));
  }

  .path {
    overflow-wrap: anywhere;
    font-size: 0.75rem;
  }

  .unknown {
    margin-top: 10px;
    padding: 8px;
    border: 1px dashed var(--line);
    border-radius: 8px;
    min-height: 60px;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .use {
    display: flex;
    flex-direction: column;
    gap: 6px;
    align-items: flex-start;
  }

  .use img {
    width: 200px;
    height: 200px;
    object-fit: cover;
    border-radius: 8px;
  }

  .check {
    display: inline-flex;
    flex-direction: row;
    align-items: center;
    gap: 6px;
  }

  .check input {
    width: auto;
    margin: 0;
    padding: 0;
    min-height: 0;
  }
</style>
