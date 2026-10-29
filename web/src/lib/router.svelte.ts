// Minimal hash router: #/search, #/album/12, ...

function current(): string {
  return location.hash.replace(/^#/, '') || '/'
}

export const route = $state({ path: current() })

// Scroll position per route, so Back returns to the same spot in a list.
const scrollPositions = new Map<string, number>()

window.addEventListener('hashchange', () => {
  scrollPositions.set(route.path, window.scrollY)
  route.path = current()
  const target = scrollPositions.get(route.path) ?? 0
  // Wait for the new screen (and any cached list) to render.
  requestAnimationFrame(() => requestAnimationFrame(() => window.scrollTo(0, target)))
})

/** Matches "/album/:id" style patterns; returns the numeric id or null. */
export function param(prefix: string): number | null {
  const m = route.path.match(new RegExp(`^${prefix}/(\\d+)$`))
  return m ? Number(m[1]) : null
}
