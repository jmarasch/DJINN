import type {
  About,
  ConvertSettings,
  Submission,
  UploadSettings,
  GoHome,
  GoPlaylist,
  GoPlaylistDetail,
  GoPrefs,
  GoResume,
  LikeKind,
  Liked,
  LikedIds,
  PlayLinks,
  Played,
  GoStatus,
  NeedsCode,
  SignIn,
  TwoStepSetup,
  TwoStepStatus,
  AccountRole,
  AccountStatus,
  Device,
  Invite,
  SignupSettings,
  AiOptions,
  AiStatus,
  Album,
  AlbumDetail,
  AlbumSort,
  ArtistDetail,
  ArtistPicture,
  ArtistSummary,
  AudioDevice,
  CastList,
  Described,
  EntryKind,
  FolderListing,
  GenreMap,
  Jump,
  GenrePage,
  GuestSummary,
  IdentificationRow,
  IdentifyStatus,
  Learned,
  LibraryStatus,
  LibraryRoot,
  Mode,
  NowPlaying,
  OutputConfig,
  OutputList,
  OutputStatus,
  Page,
  PartyStyle,
  PlayerState,
  PlaylistDetail,
  PlaylistEntry,
  PlaylistListItem,
  PlaylistSummary,
  QueueItem,
  RadioSeed,
  ReplayGainMode,
  ReplayGainSettings,
  RequestLogEntry,
  Role,
  ScanProgress,
  SearchResults,
  Settings,
  SmartPlay,
  SmartPlaySettings,
  StartResponse,
  SystemReset,
  SystemStatus,
  TagChange,
  UploadedBackup,
  Track,
  TrackSort,
  TrackEdit,
  TrackTags,
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
  login: (username: string, password: string) => post<User | NeedsCode>('/auth/login', { username, password }),
  finishTwoStep: (ticket: string, code: string) => post<User>('/auth/two-step', { ticket, code }),
  changePassword: (current: string, next: string) => post<void>('/me/password', { current, new: next }),
  twoStep: () => get<TwoStepStatus>('/me/two-step'),
  startTwoStep: () => post<TwoStepSetup>('/me/two-step/start'),
  confirmTwoStep: (code: string) => post<{ recovery_codes: string[] }>('/me/two-step/confirm', { code }),
  turnOffTwoStep: (password: string) => post<void>('/me/two-step/off', { password }),
  myDevices: () => get<Device[]>('/me/sessions'),
  appPassword: () => get<{ password: string | null }>('/me/app-password'),
  newAppPassword: () => post<{ password: string | null }>('/me/app-password'),
  removeAppPassword: () => del<void>('/me/app-password'),
  signOutMyDevice: (id: number) => del<void>(`/me/sessions/${id}`),
  signOutMyOtherDevices: () => del<void>('/me/sessions'),
  /** DJINN GO: a short-lived link to a song's file (GO port only). */
  playLinks: (trackId: number, convert = false) =>
    get<PlayLinks>(`/go/play/${trackId}${convert ? '?convert=1' : ''}`),
  convertSettings: () => get<ConvertSettings & { cache_used_mb: number }>('/go/convert'),
  saveConvertSettings: (s: ConvertSettings) => put<ConvertSettings & { cache_used_mb: number }>('/go/convert', s),
  goHome: () => get<GoHome>('/go/home'),
  goAddress: (here: string) =>
    get<{ url: string; qr_svg: string; others: string[] }>(`/go/address?here=${encodeURIComponent(here)}`),
  goLiked: () => get<Liked>('/go/likes'),
  goLikedIds: () => get<LikedIds>('/go/likes/ids'),
  goLike: (kind: LikeKind, id: number, on: boolean) =>
    on ? put<void>(`/go/likes/${kind}/${id}`, {}) : del<void>(`/go/likes/${kind}/${id}`),
  goPlayed: (trackId: number, msPlayed: number) =>
    post<void>('/go/plays', { track_id: trackId, ms_played: Math.round(msPlayed) }),
  goHistory: (offset = 0) => get<Played[]>(`/go/history?offset=${offset}`),
  goResume: () => get<GoResume | null>('/go/resume'),
  goPrefs: () => get<GoPrefs>('/go/prefs'),
  setGoPrefs: (prefs: GoPrefs) => put<void>('/go/prefs', prefs),
  goSaveResume: (queue: number[], position: number, positionMs: number) =>
    put<void>('/go/resume', { queue, position, position_ms: Math.round(positionMs) }),
  goPlaylists: () => get<GoPlaylist[]>('/go/playlists'),
  goPlaylist: (id: number) => get<GoPlaylistDetail>(`/go/playlists/${id}`),
  goCreatePlaylist: (name: string, songs: number[] = []) =>
    post<GoPlaylistDetail>('/go/playlists', { name, songs }),
  goUpdatePlaylist: (id: number, changes: { name?: string; shared?: boolean }) =>
    patch<void>(`/go/playlists/${id}`, changes),
  goDeletePlaylist: (id: number) => del<void>(`/go/playlists/${id}`),
  goAddToPlaylist: (id: number, songs: number[]) => post<void>(`/go/playlists/${id}/songs`, { songs }),
  goSetPlaylistSongs: (id: number, songs: number[]) => put<void>(`/go/playlists/${id}/songs`, { songs }),
  goRadio: (trackId: number) => get<Track[]>(`/go/radio/${trackId}`),
  mySubmissions: () => get<Submission[]>('/go/submissions'),
  withdrawSubmission: (id: number) => del<void>(`/go/submissions/${id}`),
  waitingSubmissions: () => get<Submission[]>('/submissions'),
  approveSubmission: (id: number) => post<void>(`/submissions/${id}/approve`),
  declineSubmission: (id: number, note: string) => post<void>(`/submissions/${id}/decline`, { note }),
  uploadSettings: () => get<UploadSettings>('/submissions/settings'),
  saveUploadSettings: (s: UploadSettings) => put<UploadSettings>('/submissions/settings', s),
  goSendToParty: (trackId: number) => post<{ outcome: 'added' | 'backed' }>(`/go/party/${trackId}`),
  goStatus: () => get<GoStatus>('/go/status'),
  saveGoSettings: (settings: { enabled: boolean; trusted_proxies: string; admin_two_step: boolean }) =>
    put<GoStatus>('/go/settings', settings),
  signIns: (limit = 200) => get<SignIn[]>(`/go/sign-ins?limit=${limit}`),
  turnOffUserTwoStep: (id: number) => del<void>(`/users/${id}/two-step`),
  logout: () => post<void>('/auth/logout'),
  bootstrapStatus: () => get<{ needed: boolean }>('/auth/bootstrap'),
  mode: () => get<Mode>('/auth/mode'),
  joinAsGuest: (name: string) => post<User>('/auth/guest', { name }),
  /** A User when signed in right away; { pending: true } when the account waits for approval. */
  signUp: (username: string, password: string, invite?: string) =>
    post<User | { pending: true }>('/auth/signup', { username, password, invite }),
  checkInvite: (token: string) => get<{ role: string }>(`/auth/invite/${encodeURIComponent(token)}`),
  bootstrap: (token: string, username: string, password: string) =>
    post<User>('/auth/bootstrap', { token, username, password }),

  search: (q: string) => get<SearchResults>(`/search?q=${encodeURIComponent(q)}`),
  /** The query read by the AI reader; takes a few seconds. */
  ask: (q: string) => get<Described | null>(`/search/ask?q=${encodeURIComponent(q)}`),
  artists: (offset: number) => get<Page<ArtistSummary>>(`/artists?offset=${offset}`),
  albums: (sort: AlbumSort, offset: number) => get<Page<Album>>(`/albums?sort=${sort}&offset=${offset}`),
  /** Where each letter (or year) starts in a list, for the jump strip. */
  artistJumps: () => get<Jump[]>('/artists/jumps'),
  albumJumps: (sort: AlbumSort) => get<Jump[]>(`/albums/jumps?sort=${sort}`),
  trackJumps: (sort: TrackSort) => get<Jump[]>(`/tracks/jumps?sort=${sort}`),
  playWild: () => post<void>('/playlists/wild/play'),
  playRadio: (seed: RadioSeed) => post<void>('/playlists/radio/play', seed),
  trackTags: (id: number) => get<TrackTags>(`/tracks/${id}/tags`),
  /** Takes a "Sounds like" genre off a song; it stays off when the AI listens again. */
  removeStyle: (id: number, genre: string) => del<void>(`/tracks/${id}/styles/${encodeURIComponent(genre)}`),
  restoreStyle: (id: number, genre: string) => put<void>(`/tracks/${id}/styles/${encodeURIComponent(genre)}`, {}),
  genre: (name: string) => get<GenrePage>(`/genres/${encodeURIComponent(name)}`),
  genres: () => get<GenreMap>('/genres'),
  playlistFromSongs: (name: string, trackIds: number[]) =>
    post<PlaylistSummary>('/playlists/from-songs', { name, track_ids: trackIds }),
  tracks: (sort: TrackSort, offset: number) =>
    get<Page<Track>>(`/tracks?sort=${sort}&offset=${offset}`),
  track: (id: number) => get<Track>(`/tracks/${id}`),
  similarTracks: (id: number) =>
    get<{ tracks: Track[]; analyzed: boolean; analysis_on: boolean }>(`/tracks/${id}/similar`),
  similarArtists: (id: number) => get<ArtistSummary[]>(`/artists/${id}/similar`),
  album: (id: number) => get<AlbumDetail>(`/albums/${id}`),
  artist: (id: number) => get<ArtistDetail>(`/artists/${id}`),
  /** Cover art; `size` (pixels) asks for a thumbnail. */
  /** Slideshow pictures; the first request for an artist fetches them, which takes a few seconds. */
  artistPictures: (id: number) => get<ArtistPicture[]>(`/artists/${id}/pictures`),
  artUrl: (hash: string, size?: number) => `/api/art/${hash}${size ? `?size=${size}` : ''}`,

  queue: () => get<QueueItem[]>('/queue'),
  addRequest: (trackId: number) =>
    post<QueueItem & { outcome: 'added' | 'backed' }>('/queue', { track_id: trackId }),
  skipVote: () => post<{ skipped: boolean }>('/player/skip-vote'),
  /** The live stream for Play here; null when the host turned it off. */
  listen: () => get<{ url: string | null }>('/listen'),
  trackEdit: (id: number) => get<TrackEdit>(`/tracks/${id}/edit`),
  editTracks: (change: TagChange) => put<void>('/tracks/edit', change),
  /** Renames an artist on all their songs and sets the genres of the songs
   * they lead; answers where the artist is now. */
  editArtist: (id: number, change: { name?: string | null; genre?: string | null }) =>
    put<{ id: number }>(`/artists/${id}/edit`, change),
  removeRequest: (id: number) => del<void>(`/queue/${id}`),
  queueAll: (kind: EntryKind, refId: number) =>
    post<{ added: number }>('/queue/all', { kind, ref_id: refId }),
  moveRequest: (id: number, position: number) => patch<void>(`/queue/${id}`, { position }),

  nowPlaying: () =>
    get<NowPlaying & { player: PlayerState; output: OutputStatus }>('/now-playing'),
  play: () => post<void>('/player/play'),
  pause: () => post<void>('/player/pause'),
  skip: () => post<void>('/player/skip'),
  setVolume: (volume: number) => put<void>('/player/volume', { volume }),
  setCrossfade: (secs: number) => put<void>('/player/crossfade', { secs }),

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
  /** Checks an uploaded backup and keeps it until the restore starts. */
  uploadBackup: async (file: File) => {
    const res = await fetch('/api/backup/restore', { method: 'POST', body: file })
    const data = await res.json().catch(() => null)
    if (!res.ok) throw new ApiError(res.status, data?.error ?? `Upload failed (${res.status})`)
    return data as UploadedBackup
  },
  cancelRestore: () => del<void>('/backup/restore'),
  /** Each of the backup's music folders, with where it is on this computer. */
  startRestore: (moves: [string, string][]) => post<void>('/backup/restore/start', { moves }),
  browseFolders: (path?: string) =>
    get<FolderListing>(path === undefined ? '/fs/dirs' : `/fs/dirs?path=${encodeURIComponent(path)}`),
  lookUpArtistInfo: (force = false) => post<StartResponse>('/library/artist-info', { force }),

  users: () => get<User[]>('/users'),
  createUser: (username: string, password: string, role: Role, role_id: number) =>
    post<User>('/users', { username, password, role, role_id }),
  deleteUser: (id: number) => del<void>(`/users/${id}`),
  updateUser: (
    id: number,
    changes: { unlimited?: boolean; role?: Role; role_id?: number; status?: AccountStatus; password?: string },
  ) => patch<void>(`/users/${id}`, changes),
  roles: () => get<AccountRole[]>('/roles'),
  createRole: (role: Omit<AccountRole, 'id' | 'builtin' | 'accounts'>) => post<AccountRole>('/roles', role),
  updateRole: (id: number, role: Omit<AccountRole, 'id' | 'builtin' | 'accounts'>) =>
    put<AccountRole>(`/roles/${id}`, role),
  deleteRole: (id: number) => del<void>(`/roles/${id}`),
  signupSettings: () => get<SignupSettings>('/signup'),
  saveSignupSettings: (settings: SignupSettings) => put<SignupSettings>('/signup', settings),
  invites: () => get<Invite[]>('/invites'),
  createInvite: (invite: { role_id: number; note: string; uses: number | null; days: number | null }) =>
    post<Invite & { token: string }>('/invites', invite),
  deleteInvite: (id: number) => del<void>(`/invites/${id}`),
  userDevices: (id: number) => get<Device[]>(`/users/${id}/sessions`),
  signOutUserDevice: (id: number, device: number) => del<void>(`/users/${id}/sessions/${device}`),
  signOutUserEverywhere: (id: number) => del<void>(`/users/${id}/sessions`),
  guests: () => get<GuestSummary[]>('/guests'),
  requestLog: (limit = 100) => get<RequestLogEntry[]>(`/requests/log?limit=${limit}`),
  clearRequestLog: () => del<void>('/requests/log'),

  settings: () => get<Settings>('/settings'),
  saveSettings: (settings: Settings) => put<Settings>('/settings', settings),
  smartPlay: () => get<SmartPlay>('/party'),
  saveSmartPlay: (settings: SmartPlaySettings) => put<void>('/party/settings', settings),
  createPartyStyle: (name: string) => post<PartyStyle>('/party/styles', { name }),
  renamePartyStyle: (id: number, name: string) => patch<void>(`/party/styles/${id}`, { name }),
  deletePartyStyle: (id: number) => del<void>(`/party/styles/${id}`),
  usePartyStyle: (id: number) => post<void>(`/party/styles/${id}/use`),
  clearPartyStyle: (id: number) => post<void>(`/party/styles/${id}/clear`),
  partyLearned: (id: number) => get<Learned>(`/party/styles/${id}/learned`),
  outputs: () => get<OutputList>('/outputs'),
  audioDevices: () => get<AudioDevice[]>('/outputs/devices'),
  addOutput: (device_id: string, label: string) => post<OutputConfig>('/outputs', { device_id, label }),
  updateOutput: (o: OutputConfig) =>
    put<OutputConfig>(`/outputs/${o.id}`, {
      enabled: o.enabled,
      volume_trim: o.volume_trim,
      delay_ms: o.delay_ms,
      label: o.label,
    }),
  removeOutput: (id: number) => del<void>(`/outputs/${id}`),
  replayGain: () => get<ReplayGainSettings>('/audio/replaygain'),
  identify: () => get<IdentifyStatus & { found: IdentificationRow[] }>('/identify'),
  startIdentify: () => post<void>('/identify/start'),
  stopIdentify: () => post<void>('/identify/stop'),
  about: () => get<About>('/about'),
  systemStatus: () => get<SystemStatus>('/system/status'),
  systemLog: (level: string, q: string) =>
    get<{ lines: string[] }>(`/system/log?level=${level}&q=${encodeURIComponent(q)}`),
  restart: () => post<void>('/system/restart'),
  quit: () => post<void>('/system/quit'),
  systemReset: (what: SystemReset) => post<{ restarting: boolean }>('/system/reset', { what }),
  checkForUpdate: () => post<About>('/update/check'),
  installUpdate: () => post<void>('/update/install'),
  setUpdateSettings: (s: { check_updates: boolean; auto_update: boolean }) => put<void>('/update/settings', s),
  applyIdentified: (trackIds: number[]) => post<void>('/identify/apply', { track_ids: trackIds }),
  dismissIdentified: (trackIds: number[]) => post<void>('/identify/dismiss', { track_ids: trackIds }),
  ai: () => get<AiStatus>('/ai'),
  setAi: (enabled: boolean) => put<AiStatus>('/ai', { enabled }),
  downloadAi: () => post<void>('/ai/download'),
  removeAi: () => del<void>('/ai/model'),
  cancelAiDownload: () => post<void>('/ai/download/cancel'),
  stopListening: () => post<void>('/ai/listen/stop'),
  continueListening: () => post<void>('/ai/listen/start'),
  setAiOptions: (options: AiOptions) => put<void>('/ai/options', options),
  cancelReaderDownload: () => post<void>('/ai/reader/download/cancel'),
  setReaderGpu: (enabled: boolean) => put<void>('/ai/reader/gpu', { enabled }),
  downloadGpuPack: () => post<void>('/ai/reader/gpu/download'),
  cancelGpuPack: () => post<void>('/ai/reader/gpu/download/cancel'),
  removeGpuPack: () => del<void>('/ai/reader/gpu'),
  setReader: (enabled: boolean) => put<void>('/ai/reader', { enabled }),
  downloadReader: () => post<void>('/ai/reader/download'),
  removeReader: () => del<void>('/ai/reader/model'),
  casts: () => get<CastList>('/cast'),
  discoverCasts: () => post<CastList>('/cast/discover'),
  startCast: (id: string, mode: 'audio' | 'page') =>
    post<CastList>(`/cast/${encodeURIComponent(id)}/start`, { mode }),
  stopCast: (id: string) => post<CastList>(`/cast/${encodeURIComponent(id)}/stop`),
  stopAllCasts: () => post<void>('/cast/stop-all'),
  setCastVolume: (id: string, level: number) => put<void>(`/cast/${encodeURIComponent(id)}/volume`, { level }),
  saveReplayGain: (mode: ReplayGainMode, preamp_db: number) =>
    put<ReplayGainSettings>('/audio/replaygain', { mode, preamp_db }),

  libraryStatus: () => get<LibraryStatus>('/library/status'),
  openArtFolder: () => post<void>('/library/open-art-folder'),
  rescan: () => post<StartResponse>('/library/rescan'),
}
