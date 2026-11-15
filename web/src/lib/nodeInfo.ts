// What a node is, in a few words: shared by Admin > Nodes and the Rooms page.
import type { HouseNode } from './types'

/** "windows-x86_64" as a person would say it. */
export function systemName(platform: string): string {
  const [os, arch] = platform.split('-')
  const system = os === 'windows' ? 'Windows' : os === 'linux' ? 'Linux' : os === 'macos' ? 'macOS' : os
  const chip =
    arch === 'x86_64' ? '64-bit' : arch === 'aarch64' ? 'ARM 64-bit' : arch === 'arm' ? 'ARM' : (arch ?? '')
  return chip ? `${system}, ${chip}` : system
}

export function seen(ms: number | null): string {
  if (!ms) return 'never'
  const minutes = Math.round((Date.now() - ms) / 60_000)
  if (minutes < 1) return 'just now'
  if (minutes < 60) return `${minutes} min ago`
  const hours = Math.round(minutes / 60)
  if (hours < 48) return `${hours} h ago`
  return new Date(ms).toLocaleDateString()
}

export const online = (node: HouseNode) => !!node.last_seen && Date.now() - node.last_seen < 30_000

const PAGE_NAMES: Record<string, string> = { lyrics: 'Timed lyrics', visual: 'Visualizer', tv: 'TV page' }

/** A screen setting in a few words. */
export function screenName(screen: string | undefined): string {
  if (screen === 'status') return "The node's status"
  if (screen === 'off') return 'Nothing (blank)'
  return (screen ?? 'tv')
    .split(',')
    .map((p) => PAGE_NAMES[p] ?? p)
    .join(', then ')
}

export const AI_PHASES: Record<string, string> = {
  off: 'Off',
  installing: 'Installing',
  ready: 'Ready',
  working: 'Listening',
  paused: 'Paused',
  failed: 'Stopped by a problem',
}

/** The AI helper in a few words, for its button. */
export function aiName(node: HouseNode): string {
  if (!node.ai_help) return 'Off'
  return AI_PHASES[node.ai.phase] ?? 'Starting'
}
