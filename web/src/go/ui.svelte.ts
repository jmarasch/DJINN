// What's open in DJINN GO: a song's menu and the add-to-playlist picker.

import type { Track } from '../lib/types'

export const ui = $state({
  /** The song whose menu is open. */
  menu: null as Track | null,
  /** Songs waiting to be added to a playlist. */
  adding: null as Track[] | null,
})
