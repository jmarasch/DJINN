// Laying songs on a release's tracks: what the release gives each song (the
// Album Layout window's Use this layout), and the import's automatic look-up,
// which takes a Deezer release only when it matches the album completely.
import { api } from './api'
import { albumBase } from './importLayout'
import { unlocked } from './protected.svelte'
import type { EditorField, EditorSong, Release, ReleaseHit } from './types'

export type LayoutChange = { song: EditorSong; field: EditorField; value: string }

/** Which of a release's tags to take. */
export type Take = Record<string, boolean>

/** The tags a release gives the songs laid on its tracks (`songAt(i)` is
 * the song on track i, if any). Empty values are left out. */
export function releaseChanges(
  release: Release,
  songAt: (track: number) => EditorSong | undefined,
  take: Take,
  extraKeys: string[] = [],
): LayoutChange[] {
  const discs = new Set(release.tracks.map((track) => track.disc)).size
  const changes: LayoutChange[] = []
  release.tracks.forEach((track, i) => {
    const song = songAt(i)
    if (!song) return
    const set = (field: EditorField, value: string | number | null | undefined) => {
      if (value !== null && value !== undefined && value !== '') changes.push({ song, field, value: String(value) })
    }
    if (take.titles) set('title', track.title)
    if (take.numbers) {
      if (track.number > 0) set('track_no', track.number)
      if (discs > 1 || song.disc_no) set('disc_no', track.disc)
    }
    if (take.album) set('album', release.title)
    if (take.album_artist) set('album_artist', release.artist)
    if (take.artist) set('artist', track.artist ?? release.artist)
    if (take.year) set('year', release.year)
    if (take.genre) set('genre', track.genre ?? release.genre)
    for (const name of extraKeys) set(`x:${name}` as EditorField, track.extra?.[name] ?? release.extra?.[name])
  })
  return changes
}

/** Letters and digits only, for comparing names. */
const fold = (text: string) =>
  text
    .toLowerCase()
    .normalize('NFKD')
    .replace(/[̀-ͯ]/g, '')
    .replace(/^the\s+/, '')
    .replace(/[^\p{L}\p{N}]/gu, '')

/** The value most of the songs have for a field, if any do. */
function mostCommon(songs: EditorSong[], value: (song: EditorSong) => string | null | undefined): string {
  const counts = new Map<string, number>()
  for (const song of songs) {
    const v = value(song)?.trim()
    if (v) counts.set(v, (counts.get(v) ?? 0) + 1)
  }
  return [...counts.entries()].sort((a, b) => b[1] - a[1])[0]?.[0] ?? ''
}

export interface AutoMatch {
  hit: ReleaseHit
  release: Release
  changes: LayoutChange[]
}

/** Looks the songs up on Deezer by their album and artist, and returns a
 * release only when it is the same album: the same number of tracks, every
 * song laid on one of them, and the album and artist names agreeing with
 * the tags. Anything less is left to the Album Layout window. */
export async function autoMatch(songs: EditorSong[]): Promise<AutoMatch | null> {
  if (!songs.length) return null
  const album = albumBase(mostCommon(songs, (s) => s.album))
  const artist = mostCommon(songs, (s) => s.album_artist || s.artist)
  if (!album || !artist) return null
  const hits = await unlocked(() => api.editorReleaseSearch('deezer', artist, album))
  const same = hits
    .filter((hit) => hit.tracks === songs.length && fold(hit.title) === fold(album))
    .filter((hit) => fold(hit.artist).includes(fold(artist)) || fold(artist).includes(fold(hit.artist)))
    .slice(0, 3)
  for (const hit of same) {
    const found = await unlocked(() => api.editorRelease(hit.source, hit.id, songs.map((s) => s.id), !!songs[0]?.staged))
    const { release, matched } = found
    const placed = new Set(matched.map((m) => m.id))
    if (release.tracks.length !== songs.length || placed.size !== songs.length) continue
    const onTrack = new Map(matched.map((m) => [m.track, m.id]))
    const byId = new Map(songs.map((s) => [s.id, s]))
    const take: Take = {
      titles: true,
      numbers: true,
      album: true,
      album_artist: true,
      artist: release.tracks.some((track) => !!track.artist),
      year: true,
      genre: true,
    }
    const changes = releaseChanges(release, (i) => byId.get(onTrack.get(i) ?? -1), take)
    return { hit, release, changes }
  }
  return null
}
