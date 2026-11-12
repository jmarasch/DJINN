// A page shown on a node's own screen (`?screen_for=<node id>`) follows what
// that node's settings say to show. The browser runs on the node, so
// localhost is the node's own page, for its status or a blank screen.

const NODE_PAGE = 'http://localhost:8066/node?screen=1'

/** Starts checking if this page is on a node's screen; returns how to stop.
 * In a frame on the node's own page, that page follows the setting itself. */
export function followNodeScreen(): () => void {
  const node = new URLSearchParams(location.search).get('screen_for')
  if (!node || window.parent !== window) return () => {}
  const timer = setInterval(async () => {
    try {
      const res = await fetch(`/api/nodes/${encodeURIComponent(node)}/screen`)
      // Gone from Main (kicked): the node's own page says so.
      if (res.status === 404) return location.replace(NODE_PAGE)
      if (!res.ok) return
      const { url } = (await res.json()) as { url: string | null }
      if (!url) return location.replace(NODE_PAGE)
      // A page with an order switches between the TV and lyrics pages
      // itself, so only a change of setting (the query) counts.
      const to = new URL(url)
      const same = to.searchParams.has('order')
        ? to.search === location.search
        : to.pathname + to.search === location.pathname + location.search
      if (!same) location.replace(url)
    } catch {
      // Main is away; the page's own retry shows that.
    }
  }, 5000)
  return () => clearInterval(timer)
}
