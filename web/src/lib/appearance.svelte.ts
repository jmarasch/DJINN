// The picture behind every page: the house's choice, from /api/appearance.

import { api } from './api'
import { t } from './i18n.svelte'

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
  { id: 'waves', get name() { return t('bgp.waves') }, image: '/images/listening-waves.webp' },
  { id: 'plum-vinyl', get name() { return t('bgp.plum_vinyl') }, image: '/images/backgrounds/plum-vinyl.webp' },
  { id: 'violet-vinyl', get name() { return t('bgp.violet_vinyl') }, image: '/images/backgrounds/violet-vinyl.webp' },
  { id: 'tv', get name() { return t('bgp.tv') }, image: '/images/tv-backdrop.webp' },
  { id: 'aurora', get name() { return t('bgp.aurora') }, image: '' },
  { id: 'grid', get name() { return t('bgp.grid') }, image: '' },
  { id: 'plain', get name() { return t('bgp.plain') }, image: '' },
] as const
