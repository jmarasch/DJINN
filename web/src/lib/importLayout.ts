import { t } from './i18n.svelte'
// How a folder brought into the library editor is laid out, so it can be
// split into one import per album and the folder names can fill tags the
// files are missing (album, artist, disc, year).

export type FolderLayout = 'album' | 'artist' | 'artists' | 'albums' | 'together'

export const FOLDER_LAYOUTS: { key: FolderLayout; label: string; example: string }[] = [
  { key: 'album', get label() { return t('imp.album') }, get example() { return t('imp.album_ex') } },
  { key: 'artist', get label() { return t('imp.artist') }, get example() { return t('imp.artist_ex') } },
  { key: 'artists', get label() { return t('imp.artists') }, get example() { return t('imp.artists_ex') } },
  { key: 'albums', get label() { return t('imp.albums') }, get example() { return t('imp.albums_ex') } },
  { key: 'together', get label() { return t('imp.together') }, get example() { return t('imp.together_ex') } },
]

/** What a song's folders say about it. */
export interface FolderInfo {
  /** Songs with the same group go into one import. */
  group: string
  artist: string | null
  album: string | null
  year: number | null
  disc: number | null
}

const DISC = /^(?:cd|disc|disk)\s*[-_.]?\s*(\d{1,2})\b/i

/** A disc number from a folder name like "CD2" or "Disc 1". */
export function discOf(name: string): number | null {
  const m = DISC.exec(name.trim())
  return m ? Number(m[1]) : null
}

const realYear = (y: number) => y >= 1900 && y <= 2100

/** An album folder's name without a year at its start or end, and the year:
 * "2007 - Album", "Album (2007)", "Album [2007]". */
export function albumName(folder: string): { name: string; year: number | null } {
  const name = folder.trim()
  const before = /^(\d{4})\s*[-–.]\s*(.+)$/.exec(name)
  if (before && realYear(Number(before[1]))) return { name: before[2].trim(), year: Number(before[1]) }
  const after = /^(.+?)\s*[([](\d{4})[)\]]$/.exec(name)
  if (after && realYear(Number(after[2]))) return { name: after[1].trim(), year: Number(after[2]) }
  return { name, year: null }
}

/** What `path` (a file's place under the folder chosen, the folder's own
 * name first, as the browser gives it) says, laid out as `layout` says. */
export function folderInfo(path: string, layout: FolderLayout): FolderInfo {
  const parts = path.split(/[\\/]/).filter(Boolean)
  const root = parts.length > 1 ? parts[0] : ''
  const dirs = parts.slice(1, -1)
  let artist: string | undefined
  let albumDir: string | undefined
  let rest: string[] = []
  switch (layout) {
    case 'together':
      return { group: '', artist: null, album: null, year: null, disc: null }
    case 'album':
      albumDir = root || undefined
      rest = dirs
      break
    case 'artist':
      artist = root || undefined
      albumDir = dirs[0]
      rest = dirs.slice(1)
      break
    case 'artists':
      artist = dirs[0]
      albumDir = dirs[1]
      rest = dirs.slice(2)
      break
    case 'albums':
      albumDir = dirs[0]
      rest = dirs.slice(1)
      break
  }
  // A disc folder where the album folder should be belongs to the album above.
  if (albumDir && discOf(albumDir) !== null && layout !== 'album') {
    rest = [albumDir, ...rest]
    albumDir = undefined
  }
  const album = albumDir ? albumName(albumDir) : null
  const disc = rest.map(discOf).find((d) => d !== null) ?? null
  return {
    group: `${artist ?? ''}\u0000${albumDir ?? ''}`,
    artist: artist ?? null,
    album: album?.name || null,
    year: album?.year ?? null,
    disc,
  }
}

/** A song file's name as a track number and title, for files with no
 * title tag: "01 Song", "01 - Song", "1. Song", "A1 Song" (vinyl sides
 * keep their letter out). */
export function fromFileName(name: string): { track: number | null; title: string } {
  const stem = name.replace(/^.*[\\/]/, '').replace(/\.[^.]+$/, '').trim()
  // A number counts when it's padded ("01 Song") or set off ("1 - Song",
  // "1. Song"), so a title like "99 Luftballons" keeps its number.
  const padded = /^(?:[A-D])?(0\d{1,2})\s+(.+)$/.exec(stem)
  const marked = /^(?:[A-D])?(\d{1,3})\s*[-–._)]\s*(.+)$/.exec(stem)
  const m = marked ?? padded
  if (m && Number(m[1]) > 0) return { track: Number(m[1]), title: m[2].trim() }
  return { track: null, title: stem }
}
