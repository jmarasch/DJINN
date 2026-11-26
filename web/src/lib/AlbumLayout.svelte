<script lang="ts">
  // The library editor's album layout, like Picard: pick a release of the
  // album (every one MusicBrainz has, or search MusicBrainz, Deezer or
  // Apple by hand when it found the wrong album or artist), see its track
  // list with the songs laid on it (matched by title), drag songs that
  // didn't land, or landed wrong, onto the right track from Unknown, then
  // take the release's titles, numbers, album, artists, year and cover.
  // The tags go back to the grid as changes to approve.
  import { t } from './i18n.svelte'
  import { onMount } from 'svelte'
  import { api } from './api'
  import { tagLabel } from './editorTags'
  import { releaseChanges } from './autoMatch'
  import { albumBase } from './importLayout'
  import { unlocked } from './protected.svelte'
  import { toast } from './state.svelte'
  import type { EditorField, EditorSong, Release, ReleaseHit, ReleaseSource } from './types'

  let {
    songs,
    editingFiles = false,
    onapply,
    onclose,
  }: {
    songs: EditorSong[]
    editingFiles?: boolean
    /** The tags to change, and the release's cover if it is to be used. */
    onapply: (changes: { song: EditorSong; field: EditorField; value: string }[], cover: Blob | null) => void
    onclose: () => void
  } = $props()

  const SOURCES: { key: ReleaseSource; label: string }[] = [
    { key: 'musicbrainz', label: 'MusicBrainz' },
    { key: 'deezer', label: 'Deezer' },
    { key: 'itunes', label: 'Apple Music (iTunes)' },
    { key: 'spotify', get label() { return t('al.spotify_src') } },
  ]

  // svelte-ignore state_referenced_locally
  const first = songs[0]
  let source = $state<ReleaseSource>('musicbrainz')
  let artist = $state(first?.album_artist || first?.artist || '')
  let album = $state(albumBase(first?.album || ''))
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
  let take = $state<Record<string, boolean>>({})

  type TakeItem = { key: string; label: string; detail?: string; defaultOn: boolean }

  function itemsFor(r: Release | null, includeExtra: boolean): TakeItem[] {
    if (!r) return []
    const items: TakeItem[] = []
    if (r.tracks.some((track) => track.title)) items.push({ key: 'titles', label: t('al.titles'), defaultOn: true })
    if (r.tracks.some((track) => track.number > 0)) {
      const rDiscs = new Set(r.tracks.map((track) => track.disc)).size
      items.push({
        key: 'numbers',
        label: rDiscs > 1 || songs.some((s) => s.disc_no) ? t('al.numbers_discs') : t('al.numbers'),
        defaultOn: true,
      })
    }
    if (r.title) items.push({ key: 'album', label: t('ed.album'), detail: r.title, defaultOn: true })
    if (r.artist) items.push({ key: 'album_artist', label: t('ed.album_artist'), detail: r.artist, defaultOn: true })
    if (r.tracks.some((track) => track.artist) || r.artist) {
      items.push({
        key: 'artist',
        label: t('al.each_artist'),
        detail: r.tracks.some((track) => track.artist) ? t('al.per_track') : r.artist,
        defaultOn: r.tracks.some((track) => !!track.artist),
      })
    }
    if (r.year != null) items.push({ key: 'year', label: t('ed.year'), detail: String(r.year), defaultOn: true })
    if (r.genre || r.tracks.some((track) => track.genre)) {
      items.push({
        key: 'genre',
        label: t('ed.genre'),
        detail: r.genre ?? t('al.per_track'),
        defaultOn: true,
      })
    }
    if (includeExtra) {
      const seen = new Set<string>()
      for (const [name, value] of Object.entries(r.extra ?? {})) {
        seen.add(name)
        items.push({ key: `x:${name}`, label: tagLabel(name), detail: value, defaultOn: false })
      }
      for (const track of r.tracks) {
        for (const name of Object.keys(track.extra ?? {})) {
          if (seen.has(name)) continue
          seen.add(name)
          items.push({ key: `x:${name}`, label: tagLabel(name), detail: t('al.per_track'), defaultOn: false })
        }
      }
    }
    if (r.has_cover !== false) items.push({ key: 'cover', label: t('al.cover'), defaultOn: false })
    return items
  }

  const availableItems = $derived(itemsFor(release, editingFiles))

  const byId = (id: number | null) => (id === null ? undefined : songs.find((s) => s.id === id))
  const unknown = $derived(songs.filter((s) => !laid.includes(s.id)))
  const discs = $derived(new Set(release?.tracks.map((track) => track.disc) ?? []).size)

  function length(ms: number | null | undefined): string {
    if (!ms) return ''
    const s = Math.round(ms / 1000)
    return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`
  }

  const fileName = (path: string) => path.slice(Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\')) + 1)
  /** The song's folder inside its library folder. */
  const folderOf = (song: EditorSong) => {
    const rel = song.rel_path || song.path
    // A name with no folder in it (an import's) is in no folder, not cut short.
    const cut = Math.max(rel.lastIndexOf('/'), rel.lastIndexOf('\\'))
    return cut < 0 ? '' : rel.slice(0, cut)
  }

  onMount(async () => {
    const albumId = first?.album_id
    // Not one DJINN has looked up (an import, or new to it): search for it
    // by its tags straight away, on Deezer, which needs no account and
    // answers quickly. Other sources are one choice away.
    if (albumId == null) {
      if (artist.trim() || album.trim()) {
        source = 'deezer'
        await runSearch()
      }
      return
    }
    searching = true
    try {
      const found = await unlocked(() => api.editorAlbumReleases(albumId))
      if (found.known) {
        hits = found.releases
        hitsAbout = t('al.releases')
        // Like Picard: start from the release closest in track count.
        const best = [...hits].sort(
          (a, b) => Math.abs(a.tracks - songs.length) - Math.abs(b.tracks - songs.length),
        )[0]
        if (best) await choose(best)
      } else {
        hitsAbout = t('al.unknown_mb')
      }
    } catch (e) {
      hitsAbout = e instanceof Error ? e.message : t('ed.mb_failed')
    } finally {
      searching = false
    }
  })

  async function search(e: SubmitEvent) {
    e.preventDefault()
    await runSearch()
  }

  async function runSearch() {
    searching = true
    try {
      hits = await unlocked(() => api.editorReleaseSearch(source, artist, album))
      hitsAbout = t('al.found_on', { n: hits.length, source: SOURCES.find((s) => s.key === source)?.label ?? source })
    } catch (err) {
      hits = []
      hitsAbout = err instanceof Error ? err.message : t('al.search_failed')
    } finally {
      searching = false
    }
  }

  async function choose(hit: ReleaseHit) {
    chosenHit = hit
    loadingRelease = true
    try {
      const found = await unlocked(() => api.editorRelease(hit.source, hit.id, songs.map((s) => s.id), !!songs[0]?.staged))
      release = found.release
      const next: (number | null)[] = found.release.tracks.map(() => null)
      for (const m of found.matched) next[m.track] = m.id
      laid = next
      carrying = null
      const nextTake: Record<string, boolean> = {}
      for (const item of itemsFor(found.release, editingFiles)) nextTake[item.key] = item.defaultOn
      take = nextTake
    } catch (e) {
      toast(e instanceof Error ? e.message : t('al.tracks_failed'), true)
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
    const extraKeys = availableItems.filter((i) => i.key.startsWith('x:') && take[i.key]).map((i) => i.key.slice(2))
    const changes = releaseChanges(release, (i) => byId(laid[i]), take, extraKeys)
    let cover: Blob | null = null
    if (take.cover && chosenHit) {
      const res = await fetch(api.editorReleaseCover(chosenHit.source, chosenHit.id))
      if (res.ok) cover = await res.blob()
      else if (res.status === 404) toast(t('al.no_cover'), true)
      else
        toast(
          t('al.cover_refused', { error: (await res.json().catch(() => null))?.error ?? t('al.site_refused') }),
          true,
        )
    }
    onapply(changes, cover)
  }
</script>

<section class="layout">
  <header>
    <strong>{t('al.title')}</strong>
    <span class="small muted">{t('count.songs', { n: songs.length })}{first?.album ? t('al.from', { album: albumBase(first.album) }) : ''}</span>
    <button class="small-button close" onclick={onclose}>{t('common.close')}</button>
  </header>

  <div class="cols">
    <div class="find">
      <form onsubmit={search}>
        <select bind:value={source} aria-label={t('al.where')}>
          {#each SOURCES as s (s.key)}<option value={s.key}>{s.label}</option>{/each}
        </select>
        <input bind:value={artist} placeholder={t('ed.artist')} aria-label={t('ed.artist')} />
        <input bind:value={album} placeholder={t('ed.album')} aria-label={t('ed.album')} />
        <button class="small-button" type="submit" disabled={searching}>{t('cw.search')}</button>
      </form>
      <p class="small muted">{searching ? t('al.looking') : hitsAbout}</p>
      <ul class="hits">
        {#each hits as hit (hit.source + hit.id)}
          <li>
            <button class="hit" class:on={chosenHit?.id === hit.id} onclick={() => choose(hit)}>
              <b>{hit.title}</b>
              <span class="small">{hit.artist}</span>
              <span class="small muted">
                {[hit.date, hit.country, hit.format, t('al.n_tracks', { n: hit.tracks }), hit.note].filter(Boolean).join(' · ')}
              </span>
            </button>
          </li>
        {/each}
      </ul>
    </div>

    <div class="tracks">
      {#if loadingRelease}
        <p class="small muted">{t('al.getting')}</p>
      {:else if release}
        <p class="small">
          <b>{release.title}</b> {t('al.by', { artist: release.artist ?? '' })}{release.year ? ` (${release.year})` : ''} · {t(
            'al.n_tracks',
            { n: release.tracks.length },
          )} ·
          {t('al.placed', { placed: laid.filter((x) => x !== null).length, total: songs.length })}
        </p>
        <p class="small muted">{t('al.drag')}</p>
        <table>
          <tbody>
            {#each release.tracks as track, i (i)}
              {#if discs > 1 && (i === 0 || release.tracks[i - 1].disc !== track.disc)}
                <tr class="disc"><td colspan="3">{t('ed.disc_n', { n: track.disc })}</td></tr>
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
                      <span class="small muted">{length(song.duration_ms)}{song.title !== track.title ? t('al.now', { title: song.title }) : ''}</span>
                      <span class="small muted mono path">{folderOf(song)}</span>
                    </div>
                  {:else}
                    <span class="small muted">{carrying !== null ? t('al.put_here') : t('al.drop_here')}</span>
                  {/if}
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      {:else}
        <p class="small muted">{t('al.pick_release')}</p>
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
        <div class="small"><b>{t('al.unknown')}</b> <span class="muted">{t('al.not_on_track', { n: unknown.length })}</span></div>
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
      {#if chosenHit && release?.has_cover !== false}
        <img src={api.editorReleaseCover(chosenHit.source, chosenHit.id)} alt={t('al.cover_alt')} width="200" height="200" />
      {/if}
      {#if availableItems.length}
        <div class="small"><b>{t('al.take')}</b></div>
        {#each availableItems as item (item.key)}
          <label class="check small">
            <input
              type="checkbox"
              checked={!!take[item.key]}
              onchange={(e) => (take[item.key] = e.currentTarget.checked)}
            />
            <span class="take-label">
              {item.label}{#if item.detail}<span class="muted item-detail"> · {item.detail}</span>{/if}
            </span>
          </label>
        {/each}
      {:else}
        <p class="small muted">{t('al.pick_to_see')}</p>
      {/if}
      <button class="primary" disabled={!release || laid.every((x) => x === null)} onclick={apply}>{t('al.use')}</button>
      <p class="small muted">{t('al.use_note')}</p>
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

  .take-label {
    min-width: 0;
    overflow-wrap: anywhere;
  }
</style>
