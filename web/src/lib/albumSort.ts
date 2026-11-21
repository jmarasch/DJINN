// How an artist's albums are ordered on the artist page (the party app and
// web GO alike): by release year, newest or oldest first, or A to Z.
import { byName } from './format'
import { t } from './i18n.svelte'
import type { Album } from './types'

export type AlbumSort = 'year' | 'oldest' | 'name'

/** The choices, for SortChoice. */
export function albumOrders(): { value: AlbumSort; label: string }[] {
  return [
    { value: 'year', label: t('av.newest') },
    { value: 'oldest', label: t('av.oldest') },
    { value: 'name', label: t('av.az') },
  ]
}

/** The albums in the order chosen. Albums with no year go last either way,
 * and albums of one year go A to Z. */
export function sortAlbums<A extends Pick<Album, 'title' | 'year'>>(albums: A[], sort: AlbumSort): A[] {
  const list = [...albums]
  const az = byName<A>((x) => x.title)
  if (sort === 'name') return list.sort(az)
  const later = sort === 'year' ? -1 : 1
  return list.sort((a, b) => {
    if (a.year == null || b.year == null) {
      if (a.year != null) return -1
      if (b.year != null) return 1
    } else if (a.year !== b.year) return (a.year - b.year) * later
    return az(a, b)
  })
}
