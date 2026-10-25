// Pages already loaded by infinite lists, kept while the app is open so
// coming back to a list shows it instantly at the same scroll position.

export const listCache = new Map<string, { items: unknown[]; total: number }>()

/** Call when the library changes. */
export function clearListCache() {
  listCache.clear()
}
