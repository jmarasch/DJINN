import type {
  Album,
  AlbumDetail,
  ArtistDetail,
  ArtistSummary,
  EntryKind,
  FolderListing,
  GuestSummary,
  LibraryRoot,
  Mode,
  StartResponse,
  NowPlaying,
  OutputStatus,
  Page,
  PlayerState,
  PlaylistDetail,
  PlaylistEntry,
  PlaylistListItem,
  PlaylistSummary,
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
  const isJson = res.headers.get('content-type')?.includes('application/json')
  if (res.status === 204 || !isJson) return undefined as T
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
  /** Cover art; `size` (pixels) asks for a thumbnail. */
  artUrl: (hash: string, size?: number) => `/api/art/${hash}${size ? `?size=${size}` : ''}`,

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

  playlists: () => get<PlaylistListItem[]>('/playlists'),
  playlist: (id: number) => get<PlaylistDetail>(`/playlists/${id}`),
  createPlaylist: (name: string) => post<PlaylistSummary>('/playlists', { name }),
  updatePlaylist: (id: number, changes: { name?: string; shuffle?: boolean }) =>
    patch<void>(`/playlists/${id}`, changes),
  deletePlaylist: (id: number) => del<void>(`/playlists/${id}`),
  addToPlaylist: (id: number, kind: EntryKind, refId: number) =>
    post<PlaylistEntry>(`/playlists/${id}/entries`, { kind, ref_id: refId }),
  removeFromPlaylist: (id: number, entryId: number) => del<void>(`/playlists/${id}/entries/${entryId}`),
  movePlaylistEntry: (id: number, entryId: number, position: number) =>
    patch<void>(`/playlists/${id}/entries/${entryId}`, { position }),
  playPlaylist: (id: number, fromTrackId?: number) =>
    post<void>(`/playlists/${id}/play`, { from_track_id: fromTrackId ?? null }),
  stopPlaylist: () => post<void>('/playlists/stop'),

  libraryRoots: () => get<LibraryRoot[]>('/library/roots'),
  addLibraryRoot: (path: string) => post<LibraryRoot[]>('/library/roots', { path }),
  removeLibraryRoot: (path: string) => request<LibraryRoot[]>('DELETE', '/library/roots', { path }),
  browseFolders: (path?: string) =>
    get<FolderListing>(path === undefined ? '/fs/dirs' : `/fs/dirs?path=${encodeURIComponent(path)}`),
  lookUpArtistInfo: (force = false) => post<StartResponse>('/library/artist-info', { force }),

  users: () => get<User[]>('/users'),
  createUser: (username: string, password: string, role: Role) =>
    post<User>('/users', { username, password, role }),
  deleteUser: (id: number) => del<void>(`/users/${id}`),
  updateUser: (id: number, changes: { unlimited?: boolean; role?: Role; password?: string }) =>
    patch<void>(`/users/${id}`, changes),
  guests: () => get<GuestSummary[]>('/guests'),
  requestLog: (limit = 100) => get<RequestLogEntry[]>(`/requests/log?limit=${limit}`),
  clearRequestLog: () => del<void>('/requests/log'),

  settings: () => get<Settings>('/settings'),
  saveSettings: (settings: Settings) => put<Settings>('/settings', settings),

  libraryStatus: () =>
    get<{ scanning: boolean; last_scan: ScanProgress | null; tracks: number; art_dir: string }>(
      '/library/status',
    ),
  openArtFolder: () => post<void>('/library/open-art-folder'),
  rescan: () => post<StartResponse>('/library/rescan'),
}
