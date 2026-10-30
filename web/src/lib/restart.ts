// Waiting out a restart: DJINN goes away, comes back, and the page loads
// again.

/** Polls until DJINN has gone away and come back, then reloads the page.
 * Returns a function that stops waiting. */
export function reloadAfterRestart(): () => void {
  let away = false
  const timer = setInterval(async () => {
    try {
      const res = await fetch('/api/health')
      if (res.ok && away) location.reload()
    } catch {
      away = true
    }
  }, 1500)
  return () => clearInterval(timer)
}
