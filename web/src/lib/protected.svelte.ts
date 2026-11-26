// Protected Areas: actions that change the library need the admin to unlock
// them first (password, and a two-step code at the strictest level). A
// request the server refuses with 423 opens the unlock sheet, and carries on
// once it's open.
import { api, ApiError } from './api'

type Ask = { resolve: (opened: boolean) => void }

export const unlockAsk = $state<{ current: Ask | null }>({ current: null })

/** Opens the unlock sheet; true if the admin unlocked, false if they gave up. */
export function askToUnlock(): Promise<boolean> {
  return new Promise((resolve) => {
    unlockAsk.current?.resolve(false)
    unlockAsk.current = { resolve }
  })
}

/** For a plain link to a protected download (a backup): asks to unlock first
 * unless the areas are already open, then follows it. */
export async function downloadUnlocked(href: string): Promise<void> {
  const status = await api.protectedStatus().catch(() => null)
  if (status && !status.open && !(await askToUnlock())) return
  location.href = href
}

/** Runs a protected request, asking to unlock first if the server says it's locked. */
export async function unlocked<T>(run: () => Promise<T>): Promise<T> {
  try {
    return await run()
  } catch (e) {
    if (e instanceof ApiError && e.status === 423 && (await askToUnlock())) return run()
    throw e
  }
}
