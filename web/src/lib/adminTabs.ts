// Admin sections, grouped for the side menu. Names come from the language
// files (`admin.group.*`, `admin.menu.*`), read when shown so a language
// change applies at once.

import { t } from './i18n.svelte'

export const adminGroups = [
  {
    get label() {
      return t('admin.group.overview')
    },
    tabs: [
      {
        id: 'home',
        icon: 'home',
        get label() {
          return t('admin.menu.home')
        },
      },
      {
        id: 'guide',
        icon: 'book',
        get label() {
          return t('admin.menu.guide')
        },
      },
    ],
  },
  {
    get label() {
      return t('admin.group.music')
    },
    tabs: [
      {
        id: 'library',
        icon: 'library',
        get label() {
          return t('admin.menu.library')
        },
      },
      {
        id: 'genres',
        icon: 'disc',
        get label() {
          return t('admin.menu.genres')
        },
      },
      {
        id: 'editor',
        icon: 'list',
        get label() {
          return t('admin.menu.editor')
        },
      },
      {
        id: 'rules',
        icon: 'queue-play',
        get label() {
          return t('admin.menu.rules')
        },
      },
      {
        id: 'smart',
        icon: 'similar',
        get label() {
          return t('admin.menu.smart')
        },
      },
      {
        id: 'audio',
        icon: 'playing',
        get label() {
          return t('admin.menu.audio')
        },
      },
    ],
  },
  {
    get label() {
      return t('admin.group.house')
    },
    tabs: [
      {
        id: 'rooms',
        icon: 'home',
        get label() {
          return t('admin.menu.rooms')
        },
      },
      {
        id: 'nodes',
        icon: 'radio',
        get label() {
          return t('admin.menu.nodes')
        },
      },
      {
        id: 'go',
        icon: 'headphones',
        get label() {
          return t('admin.menu.go')
        },
      },
      {
        id: 'visuals',
        icon: 'grid',
        get label() {
          return t('admin.menu.visuals')
        },
      },
    ],
  },
  {
    get label() {
      return t('admin.group.access')
    },
    tabs: [
      {
        id: 'users',
        icon: 'person',
        get label() {
          return t('admin.menu.users')
        },
      },
      {
        id: 'protected',
        icon: 'settings',
        get label() {
          return t('admin.menu.protected')
        },
      },
    ],
  },
  {
    get label() {
      return t('admin.group.system')
    },
    tabs: [
      {
        id: 'ai',
        icon: 'cloud',
        get label() {
          return t('admin.menu.ai')
        },
      },
      {
        id: 'system',
        icon: 'settings',
        get label() {
          return t('admin.menu.system')
        },
      },
    ],
  },
  {
    get label() {
      return t('admin.group.activity')
    },
    tabs: [
      {
        id: 'stats',
        icon: 'list',
        get label() {
          return t('admin.menu.stats')
        },
      },
      {
        id: 'log',
        icon: 'clock',
        get label() {
          return t('admin.menu.log')
        },
      },
      {
        id: 'serverlog',
        icon: 'book',
        get label() {
          return t('admin.menu.serverlog')
        },
      },
      {
        id: 'signins',
        icon: 'person',
        get label() {
          return t('admin.menu.signins')
        },
      },
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

/** The sub-tabs of the sections that have them, as [id, name]. */
export function adminSubtabs(group: 'library' | 'genres' | 'audio' | 'smart' | 'ai'): [string, string][] {
  switch (group) {
    case 'library':
      return [
        ['folders', t('admin.sub.folders')],
        ['settings', t('admin.sub.library_settings')],
        ['online', t('admin.sub.online')],
      ]
    case 'genres':
      return [
        ['map', t('admin.sub.genre_map')],
        ['tree', t('admin.sub.genre_tree')],
        ['containers', t('admin.sub.genre_containers')],
      ]
    case 'audio':
      return [
        ['sound', t('admin.sub.sound')],
        ['outputs', t('admin.sub.outputs')],
        ['casting', t('admin.sub.casting')],
      ]
    case 'smart':
      return [
        ['play', t('admin.menu.smart')],
        ['styles', t('admin.sub.styles')],
        ['schedule', t('admin.sub.schedule')],
      ]
    case 'ai':
      return [
        ['search', t('admin.sub.search')],
        ['listen', t('admin.sub.listen')],
        ['lyrics', t('admin.sub.lyrics')],
      ]
  }
}

/** A section's name, for search results. */
export function adminTabLabel(id: AdminTab): string {
  for (const group of adminGroups) {
    for (const tab of group.tabs) if (tab.id === id) return tab.label
  }
  return id
}
