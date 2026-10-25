import type {
  Album,
  AlbumDetail,
  ArtistDetail,
  ArtistSummary,
  GuestSummary,
  Mode,
  NowPlaying,
  OutputStatus,
  Page,
  PlayerState,
  PoolEntry,
  PoolKind,
  QueueItem,
  RequestLogEntry,
  Role,
  ScanProgress,
  SearchResults,
  Settings,
  Track,
  TrackSort,
  User,
} from './types'

export class ApiError extends Error {
  constructor(
    public status: number,
    message: string,
  ) {
    super(message)
  }
}

async function request<T>(method: string, path: string, body?: unknown): Promise<T> {
  const res = await fetch(`/api${path}`, {
    method,
    headers: body === undefined ? undefined : { 'content-type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
  })
  if (!res.ok) {
    let message = `Request failed (${res.status})`
    try {
      const data = await res.json()
      if (typeof data?.error === 'string') message = data.error
    } catch {
      // Not JSON; keep the generic message.
    }
    throw new ApiError(res.status, message)
  }
  if (res.status === 204 || res.status === 202) return undefined as T
  return res.json() as Promise<T>
}

const get = <T>(path: string) => request<T>('GET', path)
const post = <T>(path: string, body?: unknown) => request<T>('POST', path, body ?? {})
const put = <T>(path: string, body: unknown) => request<T>('PUT', path, body)
const patch = <T>(path: string, body: unknown) => request<T>('PATCH', path, body)
const del = <T>(path: string) => request<T>('DELETE', path)

export const api = {
  me: () => get<User>('/me'),
  login: (username: string, password: string) => post<User>('/auth/login', { username, password }),
  logout: () => post<void>('/auth/logout'),
  bootstrapStatus: () => get<{ needed: boolean }>('/auth/bootstrap'),
  mode: () => get<Mode>('/auth/mode'),
  joinAsGuest: (name: string) => post<User>('/auth/guest', { name }),
  bootstrap: (token: string, username: string, password: string) =>
    post<User>('/auth/bootstrap', { token, username, password }),

  search: (q: string) => get<SearchResults>(`/search?q=${encodeURIComponent(q)}`),
  artists: (offset: number) => get<Page<ArtistSummary>>(`/artists?offset=${offset}`),
  albums: (offset: number) => get<Page<Album>>(`/albums?offset=${offset}`),
  tracks: (sort: TrackSort, offset: number) =>
    get<Page<Track>>(`/tracks?sort=${sort}&offset=${offset}`),
  track: (id: number) => get<Track>(`/tracks/${id}`),
  album: (id: number) => get<AlbumDetail>(`/albums/${id}`),
  artist: (id: number) => get<ArtistDetail>(`/artists/${id}`),
  artUrl: (hash: string) => `/api/art/${hash}`,

  queue: () => get<QueueItem[]>('/queue'),
  addRequest: (trackId: number) => post<QueueItem>('/queue', { track_id: trackId }),
  removeRequest: (id: number) => del<void>(`/queue/${id}`),
  moveRequest: (id: number, position: number) => patch<void>(`/queue/${id}`, { position }),

  nowPlaying: () =>
    get<NowPlaying & { player: PlayerState; output: OutputStatus }>('/now-playing'),
  play: () => post<void>('/player/play'),
  pause: () => post<void>('/player/pause'),
  skip: () => post<void>('/player/skip'),
  setVolume: (volume: number) => put<void>('/player/volume', { volume }),

  pool: () => get<PoolEntry[]>('/pool'),
  addToPool: (kind: PoolKind, refId: number) => post<PoolEntry>('/pool', { kind, ref_id: refId }),
  removeFromPool: (id: number) => del<void>(`/pool/${id}`),

  users: () => get<User[]>('/users'),
  createUser: (username: string, password: string, role: Role) =>
    post<User>('/users', { username, password, role }),
  deleteUser: (id: number) => del<void>(`/users/${id}`),
  setUnlimited: (id: number, unlimited: boolean) => patch<void>(`/users/${id}`, { unlimited }),
  guests: () => get<GuestSummary[]>('/guests'),
  requestLog: (limit = 100) => get<RequestLogEntry[]>(`/requests/log?limit=${limit}`),

  settings: () => get<Settings>('/settings'),
  saveSettings: (settings: Settings) => put<Settings>('/settings', settings),

  libraryStatus: () =>
    get<{ scanning: boolean; last_scan: ScanProgress | null; tracks: number }>('/library/status'),
  rescan: () => post<void>('/library/rescan'),
}
