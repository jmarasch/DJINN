// Waiting out a restart: DJINN goes away, comes back, and the page loads
// again.

/** Polls until DJINN has gone away and come back, then reloads the page.
 * Returns a function that stops waiting. */
export function reloadAfterRestart(target?: string): () => void {
  let away = false
  const destination = target ? new URL(target) : new URL(location.href)
  const health = new URL('/api/health', destination)
  const timer = setInterval(async () => {
    try {
      const res = await fetch(health, {
        cache: 'no-store',
        mode: health.origin === location.origin ? 'same-origin' : 'no-cors',
      })
      if ((res.ok || res.type === 'opaque') && away) location.assign(destination.href)
    } catch {
      away = true
    }
  }, 1500)
  return () => clearInterval(timer)
}
