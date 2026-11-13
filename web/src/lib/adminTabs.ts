// Admin sections, grouped for the side menu.

export const adminGroups = [
  {
    label: 'Overview',
    tabs: [{ id: 'home', label: 'Overview', icon: 'home' }],
  },
  {
    label: 'Music and playback',
    tabs: [
      { id: 'library', label: 'Library', icon: 'library' },
      { id: 'rules', label: 'Party and queue', icon: 'queue-play' },
      { id: 'smart', label: 'Smart play', icon: 'similar' },
      { id: 'audio', label: 'Audio', icon: 'playing' },
    ],
  },
  {
    label: 'House and devices',
    tabs: [
      { id: 'rooms', label: 'Rooms', icon: 'home' },
      { id: 'nodes', label: 'Nodes', icon: 'radio' },
      { id: 'go', label: 'DJINN GO', icon: 'headphones' },
      { id: 'visuals', label: 'Visualizers', icon: 'grid' },
    ],
  },
  {
    label: 'Access',
    tabs: [{ id: 'users', label: 'Users and roles', icon: 'person' }],
  },
  {
    label: 'System',
    tabs: [
      { id: 'ai', label: 'AI', icon: 'cloud' },
      { id: 'system', label: 'System', icon: 'settings' },
    ],
  },
  {
    label: 'Activity and logs',
    tabs: [
      { id: 'stats', label: 'Stats', icon: 'list' },
      { id: 'log', label: 'Request log', icon: 'clock' },
      { id: 'serverlog', label: 'Server log', icon: 'book' },
      { id: 'signins', label: 'Sign-in log', icon: 'person' },
      { id: 'problems', label: 'Library problems', icon: 'warning' },
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
