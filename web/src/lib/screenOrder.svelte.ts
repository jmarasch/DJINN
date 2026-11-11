// A node's screen tries pages in the order its settings give (`?order=`,
// from Main's Admin > Nodes): timed lyrics while the song has them, the
// visualizer while the node's speaker is playing, and the TV page. The
// node's own page holds this one in a frame and draws the visualizer under
// it, so this page tells it what it shows and hears whether there's sound.

export type ScreenPage = 'lyrics' | 'visual' | 'tv'

const params = new URLSearchParams(location.search)
/** The pages to try, or empty when this isn't a node's screen with an order. */
export const order: ScreenPage[] = (params.get('order') ?? '')
  .split(',')
  .filter((p): p is ScreenPage => p === 'lyrics' || p === 'visual' || p === 'tv')

const framed = window.parent !== window
/** Shown on a node's own screen, where there's nowhere else to go. */
export const onNodeScreen = params.has('screen_for')

/** Full screen for the whole page: the visualizer page around this one when
 * it's in that frame, otherwise this page. */
export function toggleFullscreen() {
  let doc = document
  try {
    if (framed && window.top) doc = window.top.document
  } catch {
    // A node's page (another address): only this frame can go full screen.
  }
  if (doc.fullscreenElement) void doc.exitFullscreen()
  else doc.documentElement.requestFullscreen?.().catch(() => {})
}
/** When the node last said its speaker is playing. */
let readyAt = $state(0)
let now = $state(performance.now())

if (framed && order.length) {
  window.addEventListener('message', (e) => {
    if (e.source !== window.parent) return
    if (e.data?.djinn === 'visual-ready') readyAt = e.data.ready ? performance.now() : 0
  })
  setInterval(() => (now = performance.now()), 1000)
}

/** The node's speaker is playing, so its visualizer has something to draw. */
export function visualReady(): boolean {
  return readyAt > 0 && now - readyAt < 3000
}

/** Which page to show now: the first in the order that can, or the first if
 * none can. */
export function choose(song: { playing: boolean; timed: boolean }): ScreenPage {
  for (const page of order) {
    if (page === 'tv') return page
    if (page === 'lyrics' && song.playing && song.timed) return page
    if (page === 'visual' && song.playing && visualReady()) return page
  }
  return order[0] ?? 'tv'
}

/** Tells the node's page what this one shows, so it draws the visual only
 * when it's wanted, and the song playing, so its visualizers can take turns
 * with each song. */
export function announce(page: ScreenPage, song: number | null) {
  if (framed) window.parent.postMessage({ djinn: 'screen-mode', mode: page, song }, '*')
}

/** The same screen as another page (`/tv` or `/lyrics`), keeping its settings. */
export function pageUrl(page: 'tv' | 'lyrics'): string {
  return `/${page}${location.search}`
}
