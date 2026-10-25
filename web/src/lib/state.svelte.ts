// App-wide state, kept current by Server-Sent Events.

import { api, ApiError } from './api'
import type {
  NowPlaying,
  OutputStatus,
  PlayerState,
  QueueItem,
  ScanProgress,
  User,
} from './types'

export const app = $state({
  me: null as User | null,
  /** False until the first /api/me check finishes. */
  ready: false,
  connected: false,
  nowPlaying: { current: null, position_ms: 0, up_next: null } as NowPlaying,
  player: { state: 'idle', volume: 80 } as PlayerState,
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
    events?.addEventListener(name, (e) => handle(JSON.parse((e as MessageEvent).data)))

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
  on<ScanProgress>('scan_progress', (data) => (app.scan = data))

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

export async function logout() {
  await api.logout().catch(() => {})
  disconnect()
  app.me = null
}

export function isAdmin(): boolean {
  return app.me?.role === 'admin'
}

// Short-lived messages at the bottom of the screen.
export const toasts = $state({ items: [] as { id: number; text: string; error: boolean }[] })
let toastId = 0

export function toast(text: string, error = false) {
  const id = ++toastId
  toasts.items.push({ id, text, error })
  setTimeout(() => {
    toasts.items = toasts.items.filter((t) => t.id !== id)
  }, 3500)
}

/** Queues a track with an optimistic "pending" state until SSE confirms. */
export async function requestTrack(trackId: number) {
  app.pending = new Set(app.pending).add(trackId)
  try {
    await api.addRequest(trackId)
    toast('Added to the queue')
  } catch (e) {
    const next = new Set(app.pending)
    next.delete(trackId)
    app.pending = next
    toast(e instanceof Error ? e.message : 'Could not add that song', true)
  }
}

export function trackStatus(trackId: number): 'playing' | 'queued' | 'pending' | null {
  if (app.nowPlaying.current?.track.id === trackId) return 'playing'
  if (app.queue.some((q) => q.track.id === trackId)) return 'queued'
  if (app.pending.has(trackId)) return 'pending'
  return null
}
