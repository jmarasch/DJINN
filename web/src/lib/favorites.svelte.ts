// Your favorites in the party app: the hearts on songs, albums and
// artists (the same likes DJINN GO keeps). Guests have none.

import { api } from './api'
import { hasPlaylists, toast } from './state.svelte'
import type { LikeKind } from './types'

export const favorites = $state({
  track: new Set<number>(),
  album: new Set<number>(),
  artist: new Set<number>(),
})

let loaded: Promise<void> | null = null

/** Loads your hearts once, if you can have them. */
export function loadFavorites(): Promise<void> {
  if (!hasPlaylists()) return Promise.resolve()
  loaded ??= api
    .likeIds()
    .then((ids) => {
      favorites.track = new Set(ids.tracks)
      favorites.album = new Set(ids.albums)
      favorites.artist = new Set(ids.artists)
    })
    .catch(() => {
      loaded = null
    })
  return loaded
}

/** Hearts or un-hearts at once; puts it back if the server says no. */
export async function toggleFavorite(kind: LikeKind, id: number) {
  const on = !favorites[kind].has(id)
  const flip = (to: boolean) => {
    const next = new Set(favorites[kind])
    if (to) next.add(id)
    else next.delete(id)
    favorites[kind] = next
  }
  flip(on)
  try {
    await api.setLike(kind, id, on)
  } catch (e) {
    flip(!on)
    toast(e instanceof Error ? e.message : 'Could not save that', true)
  }
}
