// A folder to open in the library editor, or words to look for, set by
// "Fix in the editor" on another page (Library problems) and taken once by
// the editor.
export const editorJump = $state<{ folder: string; search: string }>({ folder: '', search: '' })

/** Opens the library editor at `folder`. */
export function openInEditor(folder: string) {
  editorJump.folder = folder
  editorJump.search = ''
  location.hash = '#/admin/editor'
}

/** Opens the library editor with the songs found by `words` (any of them). */
export function findInEditor(words: string[]) {
  editorJump.folder = ''
  editorJump.search = words.filter((w) => w.trim()).join(' | ')
  location.hash = '#/admin/editor'
}

/** What was asked for, once. */
export function takeEditorJump(): { folder: string; search: string } {
  const asked = { folder: editorJump.folder, search: editorJump.search }
  editorJump.folder = ''
  editorJump.search = ''
  return asked
}
