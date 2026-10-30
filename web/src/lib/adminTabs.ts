// Admin sections, in two rows: running the party, then the system.

export const adminTabRows = [
  [
    { id: 'rules', label: 'Rules' },
    { id: 'smart', label: 'Smart play' },
    { id: 'users', label: 'Users' },
    { id: 'log', label: 'Log' },
  ],
  [
    { id: 'library', label: 'Library' },
    { id: 'audio', label: 'Audio' },
    { id: 'ai', label: 'AI' },
    { id: 'system', label: 'System' },
    { id: 'about', label: 'About' },
  ],
] as const

export type AdminTab = (typeof adminTabRows)[number][number]['id']

const ids: readonly string[] = adminTabRows.flat().map((t) => t.id)

/** The tab a path like "/admin/smart" names; Library for "/admin" or anything unknown. */
export function adminTabFor(path: string): AdminTab {
  const id = path.match(/^\/admin\/([a-z]+)$/)?.[1] ?? ''
  return (ids.includes(id) ? id : 'library') as AdminTab
}
