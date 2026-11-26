// What a node is, in a few words: shared by Admin > Nodes and the Rooms page.
import type { HouseNode } from './types'
import { locale, t } from './i18n.svelte'

/** "windows-x86_64" as a person would say it. */
export function systemName(platform: string): string {
  const [os, arch] = platform.split('-')
  const system = os === 'windows' ? 'Windows' : os === 'linux' ? 'Linux' : os === 'macos' ? 'macOS' : os
  const chip =
    arch === 'x86_64' ? t('ni.64bit') : arch === 'aarch64' ? t('ni.arm64') : arch === 'arm' ? 'ARM' : (arch ?? '')
  return chip ? `${system}, ${chip}` : system
}

export function seen(ms: number | null): string {
  if (!ms) return t('ni.never')
  const minutes = Math.round((Date.now() - ms) / 60_000)
  if (minutes < 1) return t('ni.just_now')
  if (minutes < 60) return t('ni.min_ago', { n: minutes })
  const hours = Math.round(minutes / 60)
  if (hours < 48) return t('ni.h_ago', { n: hours })
  return new Date(ms).toLocaleDateString(locale.code)
}

export const online = (node: HouseNode) => !!node.last_seen && Date.now() - node.last_seen < 30_000

const PAGES = ['lyrics', 'visual', 'tv']

/** A screen setting in a few words. */
export function screenName(screen: string | undefined): string {
  if (screen === 'status') return t('ni.status')
  if (screen === 'off') return t('ni.blank')
  if (screen === '') return t('ni.room_says')
  return (screen ?? 'tv')
    .split(',')
    .map((p) => (PAGES.includes(p) ? t(`ni.page_${p}`) : p))
    .join(t('ni.then'))
}

const AI_PHASE_KEYS = ['off', 'installing', 'ready', 'working', 'paused', 'failed']

/** An AI helper's phase in a few words, or null for one this app doesn't know. */
export function aiPhase(phase: string): string | null {
  return AI_PHASE_KEYS.includes(phase) ? t(`ni.ai_${phase}`) : null
}

const AI_WHY_KEYS = ['music', 'held', 'offline', 'screen']

/** Why a node's AI helper is paused, from the word it reports (older nodes
 * send a sentence, shown as it is). */
export function aiWhy(detail: string | null | undefined): string | null {
  if (!detail) return null
  return AI_WHY_KEYS.includes(detail) ? t(`ni.ai_why_${detail}`) : detail
}

/** The AI helper in a few words, for its button. */
export function aiName(node: HouseNode): string {
  if (!node.ai_help) return t('ni.ai_off')
  return aiPhase(node.ai.phase) ?? t('ni.ai_starting')
}
