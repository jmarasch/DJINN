// App-wide state, kept current by Server-Sent Events.

import { api, ApiError } from './api'
import { clearListCache } from './listCache'
import type {
  AiStatus,
  CastDevice,
  LoudnessStatus,
  LookupStatus,
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
  player: {
    state: 'idle',
    volume: 80,
    crossfade_secs: 0,
    playlist: null,
    skip_votes: 0,
    skip_voters: [],
    skip_votes_needed: 0,
  } as PlayerState,
  output: { device: null, sample_rate: null, error: null, outputs: [] } as OutputStatus,
  queue: [] as QueueItem[],
  scan: null as ScanProgress | null,
  lookup: { running: false, last: null, finished_at: null } as LookupStatus,
  /** ReplayGain loudness measuring; filled in on the Audio tab. */
  loudness: null as LoudnessStatus | null,
  /** AI listening; filled in on the Library tab, kept current by SSE. */
  ai: null as AiStatus | null,
  /** Cast devices; filled in on the Audio tab, kept current by SSE. */
  casts: null as CastDevice[] | null,
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

/** DJINN was updated while this page was open: load the new app. */
export function reloadIfUpdated(serverBuild: string | null) {
  // Only built pages have a hashed script; `npm run dev` doesn't.
  const mine = document.querySelector('script[type="module"][src*="/assets/index-"]')?.getAttribute('src')
  if (serverBuild && mine && !mine.endsWith(serverBuild)) location.reload()
}

export function connect() {
  events?.close()
  events = new EventSource('/api/events')
  events.onopen = () => (app.connected = true)
  const on = <T>(name: string, handle: (data: T) => void) =>
    events?.addEventListener(name, (e) => {
      const data = (e as MessageEvent).data
      handle(data ? JSON.parse(data) : (null as T))
    })

  on<{ web_build: string | null }>('hello', (data) => reloadIfUpdated(data.web_build))
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
  on<LookupStatus>('artist_lookup', (data) => (app.lookup = data))
  on<LoudnessStatus>('loudness', (data) => (app.loudness = data))
  on<CastDevice[]>('cast_status', (data) => (app.casts = data))
  on<AiStatus>('ai_status', (data) => (app.ai = data))

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

/** Queues a track with an optimistic "pending" state until SSE confirms.
 * Asking for a song someone else queued backs it, moving it up a place. */
export async function requestTrack(trackId: number) {
  app.pending = new Set(app.pending).add(trackId)
  try {
    const item = await api.addRequest(trackId)
    toast(
      item.outcome === 'backed' ? 'You asked for it too, so it moved up a place' : 'Added to the queue',
      false,
      { label: 'Undo', run: () => cancelRequest(item.id) },
    )
  } catch (e) {
    const next = new Set(app.pending)
    next.delete(trackId)
    app.pending = next
    toast(e instanceof Error ? e.message : 'Could not add that song', true)
  }
}

/** How you're part of a queued song: you asked first, or asked too. */
export function involvement(item: QueueItem): 'owner' | 'backer' | null {
  if (item.user_id === app.me?.id) return 'owner'
  if (item.backers.some((b) => b.user_id === app.me?.id)) return 'backer'
  return null
}

/** Everyone who asked for a song: "you, Sam & Jo". */
export function askedBy(item: QueueItem): string {
  const me = app.me?.id
  const names = [
    item.user_id === me ? 'you' : item.username,
    ...item.backers.map((b) => (b.user_id === me ? 'you' : b.name)),
  ]
  return names.length < 2 ? names.join('') : `${names.slice(0, -1).join(', ')} & ${names[names.length - 1]}`
}

/** Takes you off a request: as a backer you step back; as the requester
 * it passes to whoever asked next (or goes). Admins remove anyone's. */
export async function cancelRequest(queueId: number) {
  const item = app.queue.find((q) => q.id === queueId)
  const role = item ? involvement(item) : null
  const message =
    role === 'backer'
      ? 'You stepped back from that song'
      : role === 'owner' && item?.backers.length
        ? `It's ${item.backers[0].name}'s request now`
        : 'Removed from the queue'
  const before = app.queue
  // Only drop it right away when it really leaves the queue.
  if (!item?.backers.length && role !== 'backer') app.queue = app.queue.filter((q) => q.id !== queueId)
  try {
    await api.removeRequest(queueId)
    toast(message)
  } catch (e) {
    app.queue = before
    toast(e instanceof Error ? e.message : 'Could not remove that song', true)
  }
}

/** A queued song you asked for (first or too), if any. */
export function myRequest(trackId: number) {
  return app.queue.find((q) => q.track.id === trackId && involvement(q) !== null) ?? null
}

/** `mine`: you asked for it, so you can step back. `queued`: someone else
 * did, so you can ask for it too. */
export function trackStatus(trackId: number): 'playing' | 'mine' | 'queued' | 'pending' | null {
  if (app.nowPlaying.current?.track.id === trackId) return 'playing'
  if (myRequest(trackId)) return 'mine'
  if (app.pending.has(trackId)) return 'pending'
  if (app.queue.some((q) => q.track.id === trackId)) return 'queued'
  return null
}

/** Tapping a song's picture or title: queue it, or ask for it too. */
export function tapTrack(track: { id: number; title: string }) {
  switch (trackStatus(track.id)) {
    case null:
    case 'queued':
      requestTrack(track.id)
      break
    case 'playing':
      toast('Playing now')
      break
    case 'mine':
      toast('You asked for this one. Tap the check to step back.')
      break
    default:
      toast('Adding it...')
  }
}

/** Queues the track (or asks for it too), or takes back your request. */
export function toggleRequest(trackId: number) {
  const mine = myRequest(trackId)
  if (mine) cancelRequest(mine.id)
  else requestTrack(trackId)
}
