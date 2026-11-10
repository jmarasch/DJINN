// Admin sections, grouped for the side menu.

export const adminGroups = [
  {
    label: 'Dashboard',
    tabs: [{ id: 'home', label: 'Dashboard' }],
  },
  {
    label: 'Settings',
    tabs: [
      { id: 'rules', label: 'Party' },
      { id: 'smart', label: 'Smart play' },
      { id: 'audio', label: 'Audio' },
      { id: 'users', label: 'Users' },
      { id: 'go', label: 'DJINN GO' },
      { id: 'rooms', label: 'Rooms' },
      { id: 'nodes', label: 'Nodes' },
      { id: 'library', label: 'Library' },
      { id: 'ai', label: 'AI' },
      { id: 'system', label: 'System' },
    ],
  },
  {
    label: 'Metrics and logs',
    tabs: [
      { id: 'stats', label: 'Stats' },
      { id: 'log', label: 'Request log' },
      { id: 'serverlog', label: 'Server log' },
      { id: 'signins', label: 'Sign-in log' },
      { id: 'problems', label: 'Library problems' },
    ],
  },
] as const

export type AdminTab = (typeof adminGroups)[number]['tabs'][number]['id']

const ids: readonly string[] = adminGroups.flatMap((g) => g.tabs.map((t) => t.id))

/** The tab a path like "/admin/smart" names; the dashboard for "/admin" or anything unknown
 * (old links to /admin/about land there too, where About now is). */
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
