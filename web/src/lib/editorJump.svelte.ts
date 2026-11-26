// The library editor lives in a window of its own (`#/editor`). A folder to
// open there, words to look for, or a tab, travel in the window's address,
// which is how "Fix in the editor" on Library Problems (a tab of the window)
// asks for them. An editor window already open is pointed at the new place
// without being closed, so it stays unlocked; inside it, the address changes
// in place.

/** Opens the editor window (or brings the one open forward), asking for a
 * folder to start at or words to find. */
export function openEditorWindow(ask: { folder?: string; search?: string; tab?: EditorTab } = {}) {
  const query = new URLSearchParams()
  if (ask.folder) query.set('folder', ask.folder)
  if (ask.search) query.set('search', ask.search)
  if (ask.tab) query.set('tab', ask.tab)
  const text = query.toString()
  const hash = `#/editor${text ? `?${text}` : ''}`
  if (location.hash.startsWith('#/editor')) {
    location.hash = hash
    return
  }
  const url = `${location.pathname}${hash}`
  const opened = window.open(url, 'djinn-library-editor', 'width=1600,height=950')
  opened?.focus()
}

/** The editor window's tabs. */
export type EditorTab = 'editor' | 'folders' | 'compare' | 'problems'

/** Opens the editor window on Library Problems. */
export function openProblems() {
  openEditorWindow({ tab: 'problems' })
}

/** Opens the library editor at `folder`. */
export function openInEditor(folder: string) {
  openEditorWindow({ folder })
}

/** Opens the library editor with the songs found by `words` (any of them). */
export function findInEditor(words: string[]) {
  openEditorWindow({ search: words.filter((w) => w.trim()).join(' | ') })
}
