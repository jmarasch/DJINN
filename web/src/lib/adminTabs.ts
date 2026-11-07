// Admin sections, grouped for the side menu.

export const adminGroups = [
  {
    label: 'Party',
    tabs: [
      { id: 'home', label: 'Home' },
      { id: 'rules', label: 'Rules' },
      { id: 'smart', label: 'Smart play' },
      { id: 'audio', label: 'Audio' },
      { id: 'stats', label: 'Stats' },
      { id: 'log', label: 'Request log' },
    ],
  },
  {
    label: 'People',
    tabs: [
      { id: 'users', label: 'Users' },
      { id: 'go', label: 'DJINN GO' },
    ],
  },
  {
    label: 'House',
    tabs: [{ id: 'nodes', label: 'Nodes' }],
  },
  {
    label: 'Library',
    tabs: [
      { id: 'library', label: 'Library' },
      { id: 'problems', label: 'Problems' },
      { id: 'ai', label: 'AI' },
    ],
  },
  {
    label: 'System',
    tabs: [
      { id: 'system', label: 'System' },
      { id: 'about', label: 'About' },
    ],
  },
] as const

export type AdminTab = (typeof adminGroups)[number]['tabs'][number]['id']

const ids: readonly string[] = adminGroups.flatMap((g) => g.tabs.map((t) => t.id))

/** The tab a path like "/admin/smart" names; Home for "/admin" or anything unknown. */
export function adminTabFor(path: string): AdminTab {
  const id = path.match(/^\/admin\/([a-z]+)$/)?.[1] ?? ''
  return (ids.includes(id) ? id : 'home') as AdminTab
}

/** A section's name, for search results. */
export function adminTabLabel(id: AdminTab): string {
  for (const group of adminGroups) {
    for (const tab of group.tabs) if (tab.id === id) return tab.label
  }
  return id
}
