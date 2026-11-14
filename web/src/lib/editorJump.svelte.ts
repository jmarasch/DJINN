// A folder to open in the library editor, set by "Fix in the editor" on
// another page (Library problems) and taken once by the editor.
export const editorJump = $state<{ folder: string }>({ folder: '' })

/** Opens the library editor at `folder`. */
export function openInEditor(folder: string) {
  editorJump.folder = folder
  location.hash = '#/admin/editor'
}

/** The folder asked for, once. */
export function takeEditorJump(): string {
  const folder = editorJump.folder
  editorJump.folder = ''
  return folder
}
