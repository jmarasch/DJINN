// Pages already loaded by infinite lists, kept while the app is open so
// coming back to a list shows it instantly at the same scroll position.
// `start` is where the loaded stretch begins (after jumping into a list).

import type { Jump } from './types'

export const listCache = new Map<string, { items: unknown[]; total: number; start: number }>()

/** Where each letter or year starts, per list and sort. */
export const jumpCache = new Map<string, Jump[]>()

/** Call when the library changes. */
export function clearListCache() {
  listCache.clear()
  jumpCache.clear()
}
