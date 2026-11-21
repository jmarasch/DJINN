// How an artist's albums are ordered on the artist page (the party app and
// web GO alike): by release year, by when they were added, or by title.
// Each has a reverse, chosen by tapping its button again.
import { byName } from './format'
import { t } from './i18n.svelte'
import type { Album } from './types'
import type { SortOption } from './SortChoice.svelte'

export type AlbumSort = 'release' | 'release_old' | 'added' | 'added_old' | 'name' | 'name_rev'

/** The buttons, for SortChoice. */
export function albumOrders(): SortOption<AlbumSort>[] {
  return [
    {
      value: 'release',
      label: t('av.sort_release'),
      hint: t('av.newest_first'),
      reverse: 'release_old',
      reverseHint: t('av.oldest_first'),
    },
    {
      value: 'added',
      label: t('av.sort_added'),
      hint: t('av.newest_first'),
      reverse: 'added_old',
      reverseHint: t('av.oldest_first'),
    },
    { value: 'name', label: t('av.az'), hint: t('av.a_to_z'), reverse: 'name_rev', reverseHint: t('av.z_to_a') },
  ]
}

/** The year an album came out: MusicBrainz's first release when it knows
 * it (tags often give a reissue's year), else the tags'. */
export function releaseYear(album: Pick<Album, 'year' | 'first_year'>): number | null {
  return album.first_year ?? album.year ?? null
}

/** The albums in the order chosen. Albums with no year (or no added time)
 * go last either way, and ties go A to Z. */
export function sortAlbums<A extends Pick<Album, 'title' | 'year' | 'added' | 'first_year'>>(albums: A[], sort: AlbumSort): A[] {
  const list = [...albums]
  const az = byName<A>((x) => x.title)
  if (sort === 'name') return list.sort(az)
  if (sort === 'name_rev') return list.sort((a, b) => az(b, a))
  const key = (a: A) => (sort === 'added' || sort === 'added_old' ? a.added : releaseYear(a)) ?? null
  const later = sort === 'release' || sort === 'added' ? -1 : 1
  return list.sort((a, b) => {
    const x = key(a)
    const y = key(b)
    if (x == null || y == null) {
      if (x != null) return -1
      if (y != null) return 1
    } else if (x !== y) return (x - y) * later
    return az(a, b)
  })
}
