import type {
  About,
  Room,
  RoomList,
  NodeAudio,
  NodeList,
  Lyrics,
  LyricsSettings,
  LyricsStatus,
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
  GoAppRelease,
  GoKeepRules,
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
  AlbumFilter,
  AlbumSort,
  ArtistSort,
  ArtistDetail,
  ArtistPicture,
  UnsplashChoice,
  ArtCacheStatus,
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
  LogSettings,
  Mode,
  NowPlaying,
  OutputConfig,
  OutputList,
  OutputStatus,
  Page,
  GoYear,
  Wish,
  SearchHistory,
  Stats,
  StatsFilter,
  HistoryCounts,
  ScheduledCheck,
  DuplicateListing,
  ProblemItem,
  ProblemKind,
  ProblemSummary,
  PartyStyle,
  PlayerState,
  ImportResult,
  ShareTarget,
  PlaylistCheck,
  PlaylistDetail,
  PlaylistEntry,
  PlaylistListItem,
  PlaylistSummary,
  QueueItem,
  RadioSeed,
  ReplayGainMode,
  ReplayGainSettings,
  EqualizerSettings,
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
  ServerPorts,
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

let unauthorized: (() => void) | null = null

/** Called when the server turns a request down for want of a sign-in. A wrong password answers the same, so check. */
export function onUnauthorized(handler: () => void) {
  unauthorized = handler
}

async function request<T>(method: string, path: string, body?: unknown): Promise<T> {
  const res = await fetch(`/api${path}`, {
    method,
    headers: body === undefined ? undefined : { 'content-type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
  })
  if (!res.ok) {
    if (res.status === 401) unauthorized?.()
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

/** A stats filter as query parameters. */
export function statsQuery(filter: StatsFilter): string {
  const params = new URLSearchParams()
  if (filter.from != null) params.set('from', String(filter.from))
  if (filter.until != null) params.set('until', String(filter.until))
  if (filter.user != null) params.set('user', String(filter.user))
  params.set('party', String(filter.party))
  params.set('go', String(filter.go))
  return params.toString()
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
  changeUsername: (username: string, currentPassword: string) =>
    post<User>('/me/username', { username, current_password: currentPassword }),
  twoStep: () => get<TwoStepStatus>('/me/two-step'),
  startTwoStep: (password: string) => post<TwoStepSetup>('/me/two-step/start', { password }),
  confirmTwoStep: (code: string) => post<{ recovery_codes: string[] }>('/me/two-step/confirm', { code }),
  turnOffTwoStep: (password: string) => post<void>('/me/two-step/off', { password }),
  myDevices: () => get<Device[]>('/me/sessions'),
  appPassword: () => get<{ exists: boolean }>('/me/app-password'),
  newAppPassword: (password: string) => post<{ password: string }>('/me/app-password', { password }),
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
  goYear: (year?: number) => get<GoYear>(year ? `/go/year?year=${year}` : '/go/year'),
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
  goRadio: (trackId: number, exclude: number[] = []) =>
    get<Track[]>(`/go/radio/${trackId}${exclude.length ? `?exclude=${exclude.join(',')}` : ''}`),
  mySubmissions: () => get<Submission[]>('/go/submissions'),
  withdrawSubmission: (id: number) => del<void>(`/go/submissions/${id}`),
  waitingSubmissions: () => get<Submission[]>('/submissions'),
  approveSubmission: (id: number) => post<void>(`/submissions/${id}/approve`),
  declineSubmission: (id: number, note: string) => post<void>(`/submissions/${id}/decline`, { note }),
  uploadSettings: () => get<UploadSettings>('/submissions/settings'),
  saveUploadSettings: (s: UploadSettings) => put<UploadSettings>('/submissions/settings', s),
  goSendToParty: (trackId: number) => post<{ outcome: 'added' | 'backed' }>(`/go/party/${trackId}`),
  goStatus: () => get<GoStatus>('/go/status'),
  goApp: () => get<GoAppRelease | null>('/go/app'),
  saveGoSettings: (settings: { encryption: boolean; keep: GoKeepRules; server_name: string; discoverable: boolean; enabled: boolean; allow_remote: boolean; allow_plain_remote: boolean; https_hostname: string; upnp: boolean; trusted_proxies: string; admin_two_step: boolean }) =>
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
  bootstrapNode: (token: string) => post<void>('/auth/bootstrap/node', { token }),
  bootstrap: (token: string, username: string, password: string) =>
    post<User>('/auth/bootstrap', { token, username, password }),

  search: (q: string) => get<SearchResults>(`/search?q=${encodeURIComponent(q)}`),
  /** The query read by the AI reader; takes a few seconds. */
  ask: (q: string) => get<Described | null>(`/search/ask?q=${encodeURIComponent(q)}`),
  trackLyrics: (id: number) => get<Lyrics | null>(`/tracks/${id}/lyrics`),
  lyricsStatus: () => get<LyricsStatus>('/lyrics/status'),
  lyricsSettings: () => get<LyricsSettings>('/lyrics/settings'),
  setLyricsSettings: (settings: LyricsSettings) => put<void>('/lyrics/settings', settings),
  recheckLyrics: () => post<void>('/lyrics/recheck'),
  rooms: () => get<RoomList>('/rooms'),
  chooseRoom: (roomId: number) => put<void>('/me/room', { room_id: roomId }),
  createRoom: (name: string) => post<Room>('/rooms', { name }),
  renameRoom: (id: number, name: string) => patch<void>(`/rooms/${id}`, { name }),
  deleteRoom: (id: number) => del<void>(`/rooms/${id}`),
  linkRoom: (id: number, to: number | null) => put<void>(`/rooms/${id}/link`, { to }),
  placeOutput: (id: number, roomId: number) => put<void>(`/outputs/${id}/room`, { room_id: roomId }),
  setNodeRoom: (id: number, roomId: number) => patch<void>(`/nodes/${id}`, { room_id: roomId }),
  roomQuiet: (id: number) => get<{ follow_main: boolean; takeover: 'now' | 'after_song' }>(`/rooms/${id}/quiet`),
  setRoomQuiet: (id: number, rules: { follow_main: boolean; takeover: 'now' | 'after_song' }) =>
    put<void>(`/rooms/${id}/quiet`, rules),
  nodes: () => get<NodeList>('/nodes'),
  setNodeJoining: (joinOpen: boolean) => put<void>('/nodes/settings', { join_open: joinOpen }),
  acceptNode: (id: number, password: string) => post<void>(`/nodes/${id}/accept`, { password }),
  renameNode: (id: number, name: string) => patch<void>(`/nodes/${id}`, { name }),
  removeNode: (id: number) => del<void>(`/nodes/${id}`),
  setNodeAudio: (id: number, audio: NodeAudio) => put<void>(`/nodes/${id}/audio`, audio),
  artists: (sort: ArtistSort, offset: number) => get<Page<ArtistSummary>>(`/artists?sort=${sort}&offset=${offset}`),
  albums: (sort: AlbumSort, filter: AlbumFilter, offset: number) => get<Page<Album>>(`/albums?sort=${sort}&filter=${filter}&offset=${offset}`),
  /** Where each letter (or year) starts in a list, for the jump strip. */
  artistJumps: (sort: ArtistSort) => get<Jump[]>(`/artists/jumps?sort=${sort}`),
  albumJumps: (sort: AlbumSort, filter: AlbumFilter) => get<Jump[]>(`/albums/jumps?sort=${sort}&filter=${filter}`),
  trackJumps: (sort: TrackSort) => get<Jump[]>(`/tracks/jumps?sort=${sort}`),
  playWild: () => post<void>('/playlists/wild/play'),
  playRadio: (seed: RadioSeed) => post<void>('/playlists/radio/play', seed),
  trackTags: (id: number) => get<TrackTags>(`/tracks/${id}/tags`),
  /** Takes a "Sounds like" genre off a song; it stays off when the AI listens again. */
  removeStyle: (id: number, genre: string) => del<void>(`/tracks/${id}/styles/${encodeURIComponent(genre)}`),
  restoreStyle: (id: number, genre: string) => put<void>(`/tracks/${id}/styles/${encodeURIComponent(genre)}`, {}),
  genre: (name: string) => get<GenrePage>(`/genres/${encodeURIComponent(name)}`),
  genreSongs: (name: string, offset: number) =>
    get<Page<Track>>(`/genres/${encodeURIComponent(name)}/songs?offset=${offset}`),
  genreArtists: (name: string, offset: number) =>
    get<Page<ArtistSummary>>(`/genres/${encodeURIComponent(name)}/artists?offset=${offset}`),
  genres: (top?: number, lines?: number) =>
    get<GenreMap>(top && lines ? `/genres?top=${top}&lines=${lines}` : '/genres'),
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
  manageArtistPictures: (id: number) => get<ArtistPicture[]>(`/artists/${id}/pictures/manage`),
  searchUnsplash: (id: number) => get<UnsplashChoice[]>(`/artists/${id}/pictures/unsplash`),
  addUnsplashPicture: (id: number, photoId: string) => post<ArtistPicture[]>(`/artists/${id}/pictures/unsplash`, { photo_id: photoId }),
  uploadArtistPicture: async (id: number, file: File, credit: string, creditUrl: string) => {
    const query = new URLSearchParams({ credit, credit_url: creditUrl })
    const res = await fetch(`/api/artists/${id}/pictures?${query}`, { method: 'POST', body: file })
    const data = await res.json().catch(() => null)
    if (!res.ok) throw new ApiError(res.status, data?.error ?? `Upload failed (${res.status})`)
    return data as ArtistPicture[]
  },
  removeArtistPicture: (id: number, hash: string) => del<ArtistPicture[]>(`/artists/${id}/pictures/${hash}`),
  orderArtistPictures: (id: number, hashes: string[]) => put<ArtistPicture[]>(`/artists/${id}/pictures/order`, hashes),
  recheckArtistPictures: (id: number) => post<ArtistPicture[]>(`/artists/${id}/pictures/recheck`),
  artCacheStatus: () => get<ArtCacheStatus>('/library/art-cache'),
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
  importPlaylist: (text: string, filename?: string, name?: string) =>
    post<ImportResult>('/playlists/import', { text, filename, name: name?.trim() || undefined }),
  youtubeKey: () => get<{ has_key: boolean }>('/playlists/youtube-key'),
  setYoutubeKey: (key: string) => put<void>('/playlists/youtube-key', { key }),
  checkPlaylist: (id: number) => post<void>(`/playlists/${id}/check`),
  playlistCheck: () => get<PlaylistCheck | null>('/playlists/check'),
  shareTargets: () => get<ShareTarget[]>('/playlists/share-targets'),
  playlistShares: (id: number) => get<ShareTarget[]>(`/playlists/${id}/shares`),
  sharePlaylist: (id: number, user: number) => put<void>(`/playlists/${id}/shares/${user}`, {}),
  unsharePlaylist: (id: number, user: number) => del<void>(`/playlists/${id}/shares/${user}`),
  updatePlaylist: (id: number, changes: { name?: string; shuffle?: boolean; shared?: boolean }) =>
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
    changes: { username?: string; unlimited?: boolean; role?: Role; role_id?: number; status?: AccountStatus; password?: string },
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
  equalizer: () => get<EqualizerSettings>('/audio/equalizer'),
  identify: () => get<IdentifyStatus & { found: IdentificationRow[] }>('/identify'),
  startIdentify: () => post<void>('/identify/start'),
  stopIdentify: () => post<void>('/identify/stop'),
  about: () => get<About>('/about'),
  systemStatus: () => get<SystemStatus>('/system/status'),
  changedSettings: () => get<string[]>('/settings/changed'),
  systemPorts: () => get<ServerPorts>('/system/ports'),
  saveSystemPorts: (ports: { party_port: number; go_port: number }) => put<ServerPorts>('/system/ports', ports),
  systemLog: (level: string, q: string) =>
    get<{ lines: string[] }>(`/system/log?level=${level}&q=${encodeURIComponent(q)}`),
  logSettings: () => get<LogSettings>('/system/log/settings'),
  saveLogSettings: (settings: LogSettings) => put<LogSettings>('/system/log/settings', settings),
  logFiles: () => get<string[]>('/system/log/files'),
  restart: () => post<void>('/system/restart'),
  quit: () => post<void>('/system/quit'),
  systemReset: (what: SystemReset) => post<{ restarting: boolean }>('/system/reset', { what }),
  checkForUpdate: () => post<About>('/update/check'),
  installUpdate: () => post<void>('/update/install'),
  setUpdateSettings: (s: { check_updates: boolean; auto_update: boolean; nightly: boolean }) =>
    put<void>('/update/settings', s),
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
  newStreamLink: () => post<CastList>('/cast/new-link'),
  setCastVolume: (id: string, level: number) => put<void>(`/cast/${encodeURIComponent(id)}/volume`, { level }),
  saveReplayGain: (mode: ReplayGainMode, preamp_db: number) =>
    put<ReplayGainSettings>('/audio/replaygain', { mode, preamp_db }),
  saveEqualizer: (settings: EqualizerSettings) => put<EqualizerSettings>('/audio/equalizer', settings),

  libraryStatus: () => get<LibraryStatus>('/library/status'),
  openArtFolder: () => post<void>('/library/open-art-folder'),
  rescan: () => post<StartResponse>('/library/rescan'),
  libraryProblems: () => get<ProblemSummary>('/library/problems'),
  libraryProblemList: (kind: ProblemKind, offset = 0) =>
    get<ProblemItem[]>(`/library/problems/${kind}?offset=${offset}&limit=100`),
  duplicates: (offset = 0) => get<DuplicateListing>(`/library/duplicates?offset=${offset}`),
  notDuplicates: (ids: number[]) => post<void>('/library/duplicates/dismiss', { ids }),
  reportSong: (id: number, reason: string, note?: string) => post<void>(`/tracks/${id}/report`, { reason, note }),
  clearSongReports: (trackId: number) => del<void>(`/library/reports/${trackId}`),
  scheduledCheck: () => get<ScheduledCheck>('/library/scheduled'),
  saveScheduledCheck: (on: boolean, hour: number) => put<ScheduledCheck>('/library/scheduled', { on, hour }),
  wishesEnabled: () => get<boolean>('/wishes/enabled'),
  addWish: (artist: string, title: string) => post<number>('/wishes', { artist, title }),
  wishes: (status: 'open' | 'got' | 'ignored') => get<Wish[]>(`/wishes?status=${status}`),
  setWish: (id: number, status: 'open' | 'got' | 'ignored') => put<void>(`/wishes/${id}`, { status }),
  deleteWish: (id: number) => del<void>(`/wishes/${id}`),
  searchHistory: (since: number, nothingFound: boolean) =>
    get<SearchHistory>(`/stats/searches?since=${since}&nothing_found=${nothingFound}`),
  clearSearchHistory: () => del<void>('/stats/searches'),
  stats: (filter: StatsFilter) => get<Stats>(`/stats?${statsQuery(filter)}`),
  recap: () => get<Stats | null>('/stats/recap'),
  historyCounts: (filter: StatsFilter) => get<HistoryCounts>(`/stats/clear?${statsQuery(filter)}`),
  clearHistory: (filter: StatsFilter) => post<HistoryCounts>('/stats/clear', filter),
  likeIds: () => get<LikedIds>('/likes/ids'),
  liked: () => get<Liked>('/likes'),
  setLike: (kind: LikeKind, id: number, on: boolean) =>
    on ? put<void>(`/likes/${kind}/${id}`, {}) : del<void>(`/likes/${kind}/${id}`),
  artistSongs: (id: number, offset = 0, limit = 100) =>
    get<{ total: number; tracks: Track[] }>(`/artists/${id}/songs?offset=${offset}&limit=${limit}`),
  hideLibraryProblems: (kinds: ProblemKind[]) => put<{ kinds: ProblemKind[] }>('/library/problems/hidden', { kinds }),
}
