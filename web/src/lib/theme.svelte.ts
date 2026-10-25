// Light/dark theme: follows the system unless this device picks one.

export type ThemeMode = 'auto' | 'light' | 'dark'

const KEY = 'djinn-theme'

function stored(): ThemeMode {
  try {
    const value = localStorage.getItem(KEY)
    return value === 'light' || value === 'dark' ? value : 'auto'
  } catch {
    return 'auto'
  }
}

export const theme = $state({ mode: stored() })

export function applyTheme() {
  const root = document.documentElement
  if (theme.mode === 'auto') delete root.dataset.theme
  else root.dataset.theme = theme.mode
  const dark =
    theme.mode === 'dark' ||
    (theme.mode === 'auto' && matchMedia('(prefers-color-scheme: dark)').matches)
  document.querySelector('meta[name="theme-color"]')?.setAttribute('content', dark ? '#14111f' : '#f6f4fb')
}

/** Auto, then light, then dark. */
export function cycleTheme() {
  theme.mode = theme.mode === 'auto' ? 'light' : theme.mode === 'light' ? 'dark' : 'auto'
  try {
    if (theme.mode === 'auto') localStorage.removeItem(KEY)
    else localStorage.setItem(KEY, theme.mode)
  } catch {
    // Not stored; applies for this visit only.
  }
  applyTheme()
}

matchMedia('(prefers-color-scheme: dark)').addEventListener('change', applyTheme)
