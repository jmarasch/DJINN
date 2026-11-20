// The library editor lives in a window of its own (`#/editor`). A folder to
// open there, or words to look for, travel in the window's address, which
// is how "Fix in the editor" on another page (Library Problems) asks for
// them. An editor window already open is pointed at the new place without
// being closed, so it stays unlocked.

/** Opens the editor window (or brings the one open forward), asking for a
 * folder to start at or words to find. */
export function openEditorWindow(ask: { folder?: string; search?: string } = {}) {
  const query = new URLSearchParams()
  if (ask.folder) query.set('folder', ask.folder)
  if (ask.search) query.set('search', ask.search)
  const text = query.toString()
  const url = `${location.pathname}#/editor${text ? `?${text}` : ''}`
  const opened = window.open(url, 'djinn-library-editor', 'width=1600,height=950')
  opened?.focus()
}

/** Opens the library editor at `folder`. */
export function openInEditor(folder: string) {
  openEditorWindow({ folder })
}

/** Opens the library editor with the songs found by `words` (any of them). */
export function findInEditor(words: string[]) {
  openEditorWindow({ search: words.filter((w) => w.trim()).join(' | ') })
}
