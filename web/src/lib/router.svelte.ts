// Minimal hash router: #/search, #/album/12, ...

function current(): string {
  return location.hash.replace(/^#/, '') || '/'
}

export const route = $state({ path: current() })

window.addEventListener('hashchange', () => {
  route.path = current()
  window.scrollTo(0, 0)
})

export function navigate(path: string) {
  location.hash = path
}

/** Matches "/album/:id" style patterns; returns the numeric id or null. */
export function param(prefix: string): number | null {
  const m = route.path.match(new RegExp(`^${prefix}/(\\d+)$`))
  return m ? Number(m[1]) : null
}
