// App-wide state, kept current by Server-Sent Events.

import { api, ApiError } from './api'
import { clearListCache } from './listCache'
import type {
  Mode,
  NowPlaying,
  OutputStatus,
  PlayerState,
  QueueItem,
  ScanProgress,
  User,
} from './types'

export const app = $state({
  me: null as User | null,
  /** How people get in (guest mode or accounts). */
  mode: null as Mode | null,
  /** Show the account sign-in form even in guest mode (host sign-in). */
  hostLogin: false,
  /** First-run setup created the admin; next, pick music folders. */
  setupLibrary: false,
  /** False until the first /api/me check finishes. */
  ready: false,
  connected: false,
  nowPlaying: { current: null, position_ms: 0, up_next: null } as NowPlaying,
  player: { state: 'idle', volume: 80, playlist: null } as PlayerState,
  output: { device: null, sample_rate: null, error: null } as OutputStatus,
  queue: [] as QueueItem[],
  scan: null as ScanProgress | null,
  /** Track ids with a request on its way to the server. */
  pending: new Set<number>(),
})

/** Position at `positionAt` (performance.now()); extrapolated while playing. */
let positionMs = 0
let positionAt = 0

function setPosition(ms: number) {
  positionMs = ms
  positionAt = performance.now()
}

export function currentPosition(): number {
  if (app.player.state !== 'playing') return positionMs
  return positionMs + (performance.now() - positionAt)
}

let events: EventSource | null = null

export function connect() {
  events?.close()
  events = new EventSource('/api/events')
  events.onopen = () => (app.connected = true)
  const on = <T>(name: string, handle: (data: T) => void) =>
    events?.addEventListener(name, (e) => {
      const data = (e as MessageEvent).data
      handle(data ? JSON.parse(data) : (null as T))
    })

  on<NowPlaying>('now_playing', (data) => {
    app.nowPlaying = data
    setPosition(data.position_ms)
  })
  on<{ track_id: number; position_ms: number }>('position', (data) => setPosition(data.position_ms))
  on<QueueItem[]>('queue_changed', (data) => {
    app.queue = data
    app.pending = new Set()
  })
  on<PlayerState>('player_state', (data) => {
    // Keep the extrapolated position when pausing or resuming.
    setPosition(currentPosition())
    app.player = data
  })
  on<OutputStatus>('output_status', (data) => (app.output = data))
  on<ScanProgress>('scan_progress', (data) => {
    app.scan = data
    if (data.done) clearListCache()
  })
  on('library_changed', () => clearListCache())

  events.onerror = async () => {
    app.connected = false
    // EventSource retries on its own after network errors, but gives up on
    // an HTTP error such as an expired session.
    if (events?.readyState === EventSource.CLOSED) {
      if (await refreshMe()) setTimeout(connect, 2000)
    }
  }
}

export function disconnect() {
  events?.close()
  events = null
  app.connected = false
}

export async function refreshMe(): Promise<User | null> {
  try {
    app.me = await api.me()
  } catch (e) {
    if (e instanceof ApiError && e.status === 401) app.me = null
  }
  app.ready = true
  return app.me
}

export async function refreshMode(): Promise<Mode | null> {
  try {
    app.mode = await api.mode()
  } catch {
    app.mode = null
  }
  return app.mode
}

/** Leaves the current session and shows the account sign-in form. */
export async function signInAsHost() {
  await logout()
  app.hostLogin = true
}

export async function logout() {
  await api.logout().catch(() => {})
  disconnect()
  app.me = null
}

export function isAdmin(): boolean {
  return app.me?.role === 'admin'
}

// Short-lived messages at the bottom of the screen, optionally with one
// action button (like Undo).
export interface ToastAction {
  label: string
  run: () => void
}

export const toasts = $state({
  items: [] as { id: number; text: string; error: boolean; action?: ToastAction }[],
})
let toastId = 0

export function toast(text: string, error = false, action?: ToastAction) {
  const id = ++toastId
  toasts.items.push({ id, text, error, action })
  setTimeout(() => dismissToast(id), action ? 5000 : 3500)
}

export function dismissToast(id: number) {
  toasts.items = toasts.items.filter((t) => t.id !== id)
}

/** Queues a track with an optimistic "pending" state until SSE confirms. */
export async function requestTrack(trackId: number) {
  app.pending = new Set(app.pending).add(trackId)
  try {
    const item = await api.addRequest(trackId)
    toast('Added to the queue', false, { label: 'Undo', run: () => cancelRequest(item.id) })
  } catch (e) {
    const next = new Set(app.pending)
    next.delete(trackId)
    app.pending = next
    toast(e instanceof Error ? e.message : 'Could not add that song', true)
  }
}

/** Removes one of your own requests (admins can remove any). */
export async function cancelRequest(queueId: number) {
  const before = app.queue
  app.queue = app.queue.filter((q) => q.id !== queueId)
  try {
    await api.removeRequest(queueId)
    toast('Removed from the queue')
  } catch (e) {
    app.queue = before
    toast(e instanceof Error ? e.message : 'Could not remove that song', true)
  }
}

/** Your own queued request for a track, if any. */
export function myRequest(trackId: number) {
  return app.queue.find((q) => q.track.id === trackId && q.user_id === app.me?.id) ?? null
}

/** `mine`: queued by you, so you can cancel it. */
export function trackStatus(trackId: number): 'playing' | 'mine' | 'queued' | 'pending' | null {
  if (app.nowPlaying.current?.track.id === trackId) return 'playing'
  if (myRequest(trackId)) return 'mine'
  if (app.queue.some((q) => q.track.id === trackId)) return 'queued'
  if (app.pending.has(trackId)) return 'pending'
  return null
}

/** Tapping a song's picture or title: queue it if it isn't already. */
export function tapTrack(track: { id: number; title: string }) {
  switch (trackStatus(track.id)) {
    case null:
      requestTrack(track.id)
      break
    case 'playing':
      toast('Playing now')
      break
    case 'mine':
      toast('Already in the queue. Tap the check to cancel.')
      break
    default:
      toast('Already in the queue')
  }
}

/** Queues the track, or cancels your request if it's already queued. */
export function toggleRequest(trackId: number) {
  const mine = myRequest(trackId)
  if (mine) cancelRequest(mine.id)
  else requestTrack(trackId)
}
