// Genre containers (Admin > Genres > Containers): which container each genre
// is in, while they're on, so genre chips can show the container first.
// Empty while containers are off.

import { api } from './api'

export const genreFold = $state<{ map: Record<string, string> }>({ map: {} })

let loaded: Promise<void> | null = null

/** Loads the containers once (again with `fresh`, after an admin changes them). */
export function loadGenreFold(fresh = false): Promise<void> {
  if (fresh) loaded = null
  loaded ??= api
    .genreFold()
    .then((map) => {
      genreFold.map = map
    })
    .catch(() => {
      loaded = null
    })
  return loaded
}

/** The containers of these genres, once each, in the genres' order. */
export function containersOf(genres: string[]): string[] {
  const out: string[] = []
  for (const g of genres) {
    const c = genreFold.map[g.trim().toLowerCase()] ?? genreFold.map[g.trim()]
    if (c && !out.includes(c)) out.push(c)
  }
  return out
}
