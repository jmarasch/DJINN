// The picture behind every page: the house's choice, from /api/appearance.

import { api } from './api'

export const appearance = $state({ background: 'waves', strength: 28, loaded: false })

export async function loadAppearance() {
  try {
    const found = await api.appearance()
    appearance.background = found.background
    appearance.strength = found.strength
  } catch {
    // Keep the default picture.
  }
  appearance.loaded = true
}

/** The built-in backgrounds, for the picker, with their pictures where they have one. */
export const backgrounds = [
  { id: 'waves', name: 'Waves', image: '/images/listening-waves.webp' },
  { id: 'plum-vinyl', name: 'Plum vinyl', image: '/images/backgrounds/plum-vinyl.webp' },
  { id: 'violet-vinyl', name: 'Violet vinyl', image: '/images/backgrounds/violet-vinyl.webp' },
  { id: 'tv', name: 'Stage lights', image: '/images/tv-backdrop.webp' },
  { id: 'aurora', name: 'Aurora', image: '' },
  { id: 'grid', name: 'Grid', image: '' },
  { id: 'plain', name: 'Plain', image: '' },
] as const
