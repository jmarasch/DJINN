// Mirrors the server's JSON.

export type Role = 'admin' | 'user' | 'guest'

export interface User {
  id: number
  username: string
  /** Set for guests; shown instead of username. */
  display_name: string | null
  role: Role
  /** Exempt from request limits. */
  unlimited: boolean
  mac: string | null
  created_at: number
  /** The account's role (see AccountRole); used when role is 'user'. */
  role_id: number
  status: AccountStatus
  /** Two-step sign-in is on. */
  two_step: boolean
  /** Only on the signed-in user (/me). */
  role_name?: string | null
  permissions?: Permissions
  /** DJINN GO asks this account for two-step sign-in. Only on /me. */
  two_step_required?: boolean
}

/** Login's answer when the password was right but a code is needed. */
export interface NeedsCode {
  two_step: true
  ticket: string
}

export interface TwoStepStatus {
  on: boolean
  required: boolean
  recovery_codes_left: number
}

export interface TwoStepSetup {
  secret: string
  uri: string
  qr_svg: string
}

/** DJINN GO: a song's file links. */
export interface PlayLinks {
  url: string
  download_url: string | null
  /** The file's type, to check this device can play it. */
  mime: string
  /** Volume for even loudness, 1 for unchanged. */
  gain: number
  /** `url` plays an MP3 converted from the original. */
  converted: boolean
  /** Converting is on for this account (ask with convert). */
  can_convert: boolean
}

export interface ConvertSettings {
  enabled: boolean
  kbps: number
  jobs: number
  cache_mb: number
}

export interface GoPlaylist {
  id: number
  user_id: number
  owner: string
  name: string
  shared: boolean
  tracks: number
  updated_at: number
}

export interface GoPlaylistDetail extends GoPlaylist {
  songs: Track[]
}

/** Your DJINN GO preferences. */
export interface GoPrefs {
  /** What tapping a song does: play it now, or add it to the queue. */
  tap: 'play' | 'queue'
}

export interface GoResume {
  queue: number[]
  position: number
  position_ms: number
  updated_at: number
  songs: Track[]
}

export interface GoHome {
  resume: GoResume | null
  recent: Track[]
  playlists: GoPlaylist[]
  newest: Track[]
}

export type LikeKind = 'track' | 'album' | 'artist'

export interface LikedIds {
  tracks: number[]
  albums: number[]
  artists: number[]
}

export interface Liked {
  tracks: Track[]
  albums: Album[]
  artists: ArtistSummary[]
}

export interface Played extends Track {
  played_at: number
}

export interface Submission {
  id: number
  user_id: number | null
  uploader: string | null
  file_name: string
  size: number
  title: string | null
  artist: string | null
  album: string | null
  duration_ms: number | null
  duplicate_of: number | null
  /** "Artist - Title" of the library song it looks like. */
  duplicate_title: string | null
  status: 'waiting' | 'approved' | 'declined'
  /** Why it was turned down. */
  note: string | null
  created_at: number
  decided_at: number | null
}

export interface UploadSettings {
  folder: string
  max_mb: number
}

export interface GoListener {
  username: string
  title: string
  seconds_ago: number
}

export interface GoStatus {
  enabled: boolean
  running: boolean
  error: string | null
  port: number
  addresses: string[]
  tailscale: string | null
  trusted_proxies: string
  admin_two_step: boolean
  listening: GoListener[]
  mbps: number
}

export interface SignIn {
  id: number
  at: number
  user_id: number | null
  username: string
  ip: string | null
  user_agent: string | null
  via: 'party' | 'go'
  outcome: string
}

export type AccountStatus = 'active' | 'pending' | 'disabled'

/** What an account may do. */
export interface Permissions {
  go: boolean
  go_remote: boolean
  /** 0 is no limit. */
  max_streams: number
  downloads: boolean
  share_playlists: boolean
  submit_music: boolean
  submit_without_review: boolean
  unlimited_requests: boolean
  /** Must use two-step sign-in for DJINN GO. */
  require_two_step: boolean
  /** May have songs converted, when converting is on. */
  convert: boolean
}

export interface AccountRole extends Permissions {
  id: number
  name: string
  /** Listener: editable, never deleted. */
  builtin: boolean
  accounts: number
}

export type SignupMode = 'closed' | 'review' | 'open'

export interface SignupSettings {
  mode: SignupMode
  role_id: number
}

export interface Invite {
  id: number
  role_id: number
  role_name: string
  note: string
  /** null: no limit. */
  uses_left: number | null
  used: number
  created_at: number
  /** null: never. */
  expires_at: number | null
}

/** A signed-in device. */
export interface Device {
  id: number
  created_at: number
  last_seen_at: number
  expires_at: number
  ip: string | null
  user_agent: string | null
  /** The device asking. */
  current: boolean
}

export interface Mode {
  guest_mode: boolean
  setup_needed: boolean
  /** Name this device used last time, if recognized. */
  welcome_back: string | null
  /** Whether people can create their own accounts. */
  signup: SignupMode
}

export interface GuestSummary {
  id: number
  name: string
  mac: string | null
  names: string
  last_ip: string | null
  first_seen: number
  last_seen: number
  requests: number
  unlimited: boolean
}

export interface RequestLogEntry {
  at: number
  track_id: number
  title: string | null
  artist: string | null
  user_id: number | null
  name: string | null
  mac: string | null
  ip: string | null
}

export interface Credit {
  id: number
  name: string
  featured: boolean
}

export interface Track {
  id: number
  title: string
  /** The credit as tagged, like "Zara Larsson & David Guetta". */
  artist: string | null
  /** Each credited artist, in order. */
  credits: Credit[]
  album: string | null
  album_artist: string | null
  artist_id: number | null
  album_id: number | null
  track_no: number | null
  disc_no: number | null
  year: number | null
  genre: string | null
  duration_ms: number | null
  art_hash: string | null
  /** A genre or two, in the library's song list. */
  genres?: string[]
}

export interface Album {
  id: number
  title: string
  artist_id: number
  artist: string
  year: number | null
  art_hash: string | null
  /** Comma separated, from MusicBrainz. */
  genres: string | null
}

export interface Artist {
  id: number
  name: string
  art_hash: string | null
}

export interface SearchResults {
  tracks: Track[]
  albums: Album[]
  artists: Artist[]
  /** The query read as a description ("80's hair metal"), if it is one. */
  described: Described | null
  /** The AI reader might understand the query better (see api.ask). */
  ask: boolean
}

export interface Described {
  /** What was understood, like ["1980s", "hair metal"]. */
  summary: string[]
  /** Words that weren't understood. */
  unknown: string[]
  tracks: Track[]
  /** Songs that fit in all (more than shown). */
  total: number
  artists: ArtistSummary[]
  /** The AI reader read the query. */
  read_by_ai: boolean
}

export interface AlbumDetail extends Album {
  tracks: Track[]
}

export interface TopTrack extends Track {
  requests: number
  plays: number
}

export interface ArtistDetail extends Artist {
  bio: string | null
  bio_url: string | null
  genres: string | null
  /** Credit for a free-licensed photo. */
  photo_credit: string | null
  photo_credit_url: string | null
  top_tracks: TopTrack[]
  albums: Album[]
  /** Other artists' albums this artist is credited on. */
  appears_on: Album[]
  other_tracks: Track[]
}

export interface QueueItem {
  id: number
  user_id: number
  username: string
  added_at: number
  /** Others who asked for it too, in order. */
  backers: Backer[]
  track: Track
}

export interface Backer {
  user_id: number
  name: string
}

export type PlaySource = 'request' | 'pool'

export interface Entry {
  track: Track
  source: PlaySource
  requested_by: string | null
  requested_by_id: number | null
  /** Others who asked for it too. */
  backers: string[]
}

export interface NowPlaying {
  current: Entry | null
  position_ms: number
  up_next: Entry | null
}

export type PlayState = 'idle' | 'playing' | 'paused'

export interface PlaylistRef {
  id: number
  name: string
  shuffle: boolean
}

export interface PlayerState {
  state: PlayState
  volume: number
  /** Seconds; 0 joins songs gaplessly. */
  crossfade_secs: number
  /** Plays when nobody has requested anything. */
  playlist: PlaylistRef | null
  /** Votes to skip the song playing. */
  skip_votes: number
  /** Who voted. */
  skip_voters: number[]
  /** Votes that skip it; 0 when skip voting is off. */
  skip_votes_needed: number
  /** Voted to skip the last song, so sit this one out. */
  skip_resting: number[]
}

export interface OutputStatus {
  /** The devices playing, joined. */
  device: string | null
  sample_rate: number | null
  /** Why nothing plays, when no output does. */
  error: string | null
  outputs: LiveOutput[]
}

export interface LiveOutput {
  id: number
  state: 'disabled' | 'waiting' | 'playing'
  device: string | null
  sample_rate: number | null
  error: string | null
  /** The output the others are kept in time with. */
  clock: boolean
  drift_ppm: number
  offset_ms: number
}

export interface OutputConfig {
  id: number
  /** Empty for the system default device. */
  device_id: string
  label: string
  enabled: boolean
  volume_trim: number
  delay_ms: number
}

export interface OutputList {
  outputs: OutputConfig[]
  status: OutputStatus
  max_delay_ms: number
}

export interface AudioDevice {
  id: string
  name: string
  is_default: boolean
  added: boolean
}

/** The steps of a library check, in order. */
export type ScanPhase = 'finding' | 'reading' | 'sorting' | 'done'

export interface ScanProgress {
  phase: ScanPhase
  found: number
  to_read: number
  read: number
  removed: number
  done: boolean
}

export type EntryKind = 'track' | 'album' | 'artist'

export interface PlaylistSummary {
  id: number
  name: string
  shuffle: boolean
  entry_count: number
  created_at: number
  updated_at: number
}

export interface PlaylistListItem extends PlaylistSummary {
  active: boolean
}

export interface PlaylistEntry {
  id: number
  kind: EntryKind
  ref_id: number
  name: string | null
  subtitle: string | null
  art_hash: string | null
  position: number
}

export interface PlaylistDetail extends PlaylistSummary {
  entries: PlaylistEntry[]
  tracks: Track[]
}

export interface LibraryRoot {
  path: string
  from_config: boolean
  available: boolean
}

export interface Folder {
  name: string
  path: string
}

export interface FolderListing {
  path: string | null
  parent: string | null
  dirs: Folder[]
  places: Folder[]
}

export interface Settings {
  history_window: number
  per_user_limit: number
  /** 0 means no cap. */
  max_queue: number
  guest_mode: boolean
  online_artist_info: boolean
  /** Sent to MusicBrainz, which needs one. */
  musicbrainz_contact: string
  /** Album covers, genres and exact artist matches from MusicBrainz. */
  musicbrainz: boolean
  /** Analyze every song's sound after library checks. */
  sound_analysis: boolean
  /** Slideshows of more artist pictures (Wikimedia Commons). */
  artist_pictures: boolean
  picture_settings: PictureSettings
  /** When sound analysis and AI listening may run. */
  background: Schedule
  /** Artist names never split, one per line. */
  artist_keep_whole: string
  /** Show artists who appear only as a secondary song credit. */
  show_collaboration_artists: boolean
  /** The TV page (/tv) works without signing in. */
  tv_page: boolean
  /** Anyone signed in can play the party in their own browser. */
  play_here: boolean
  /** Votes that skip a song; 0 turns skip voting off. */
  skip_votes: number
  /** AcoustID application key, for identifying songs by sound. */
  acoustid_key: string
}

export interface PictureSettings {
  count: number
  /** 0 means no separate per-artist size limit. */
  per_artist_mb: number
  /** 0 means no cache limit. */
  cache_mb: number
  max_edge: number
  min_width: number
  min_height: number
  deezer: boolean
  wikipedia: boolean
  commons: boolean
  fanart: boolean
  fanart_key: string
  audiodb: boolean
  audiodb_key: string
  unsplash: boolean
  unsplash_key: string
  allow_remote: boolean
}

export interface ArtCacheStatus {
  used_bytes: number
  max_bytes: number
}

export interface UnsplashChoice {
  id: string
  thumbnail: string
  credit: string
  credit_url: string
  width: number
  height: number
}

export interface Page<T> {
  items: T[]
  total: number
}

export interface ArtistSummary {
  id: number
  name: string
  art_hash: string | null
  track_count: number
}

/** Year is newest first, oldest is by year oldest first. */
export type TrackSort = 'title' | 'artist' | 'added' | 'popular' | 'year' | 'oldest'

/** Year is newest first, oldest is by year oldest first. */
export type AlbumSort = 'name' | 'year' | 'oldest'
export type AlbumFilter = 'all' | 'various' | 'soundtracks'

/** Where a letter or year starts in a sorted list. */
export interface Jump {
  label: string
  offset: number
}

export interface LookupSummary {
  checked: number
  photos: number
  bios: number
  covers: number
  genres: number
}

export interface LookupStatus {
  running: boolean
  last: LookupSummary | null
  finished_at: number | null
}

/** What asking for background work did. */
export interface StartResponse {
  status: 'started' | 'running' | 'queued'
}

export type ReplayGainMode = 'off' | 'track' | 'album'

/** AI listening, for the library's progress bar. */
export interface Listening {
  /** Switched on with the model installed. */
  on: boolean
  running: boolean
  heard: number
  waiting: number
  held: Held | null
}

/** When heavy background work may run; hours are this computer's time. */
export interface Schedule {
  when: 'any' | 'idle' | 'hours'
  from: number
  until: number
}

/** Why background work is waiting. */
export type Held = { reason: 'music' } | { reason: 'hours'; from: number; until: number }

export interface LibraryStatus {
  scanning: boolean
  last_scan: ScanProgress | null
  tracks: number
  art_dir: string
  /** Sound analysis, then AI listening; artist lookups alongside. */
  analysis: LoudnessStatus
  listening: Listening
  lookup_running: boolean
}

export interface LoudnessStatus {
  /** Songs whose loudness is known (tags or measured). */
  known: number
  /** Songs still to analyze. */
  waiting: number
  /** Songs analyzed (loudness and sound). */
  analyzed: number
  running: boolean
  /** Sound analysis is switched on. */
  on: boolean
  held: Held | null
}

export interface ReplayGainSettings {
  mode: ReplayGainMode
  preamp_db: number
  max_preamp_db: number
  loudness: LoudnessStatus
}

export interface EqualizerSettings {
  enabled: boolean
  /** Bass, low mids, mids, presence, and treble in dB. */
  bands_db: [number, number, number, number, number]
}

export interface CastDevice {
  id: string
  name: string
  kind: 'cast' | 'dlna' | 'sonos'
  model: string | null
  /** A Google Cast speaker group (plays in sync on every speaker in it). */
  group: boolean
  /** Has a screen, so it can show the TV page. */
  screen: boolean
  /** DJINN is casting to it, or trying to. */
  active: boolean
  /** Showing the TV page (with sound), not just audio. */
  page: boolean
  playing: boolean
  buffering: boolean
  volume: number | null
  /** The device sets its own volume (a TV remote). */
  volume_fixed: boolean
  error: string | null
}

export interface CastList {
  devices: CastDevice[]
  listeners: number
}

export interface AiStatus {
  installed: boolean
  enabled: boolean
  /** Loaded and ready. */
  ready: boolean
  /** Bytes. */
  download_size: number
  /** [done, total] bytes while downloading. */
  downloading: [number, number] | null
  error: string | null
  heard: number
  waiting: number
  listening: boolean
  held: Held | null
  reader: ReaderStatus
  options: AiOptions
  system: SystemInfo
}

export interface AiOptions {
  listen_speed: 'gentle' | 'normal' | 'fast'
  /** Songs with no genres get the ones the AI hears. */
  genre_tags: boolean
  /** Whose searches the AI reader reads. */
  reader_for: 'everyone' | 'users' | 'admins'
}

/** What this computer has. */
export interface SystemInfo {
  cores: number
  /** Bytes; null where DJINN can't tell. */
  memory_total: number | null
  memory_available: number | null
}

/** Where a radio starts from. */
export type RadioSeed = { kind: 'track'; id: number } | { kind: 'artist'; id: number } | { kind: 'genre'; name: string }

/** A song's genres (tags, album, artists) and styles (AI listening). */
export interface TrackTags {
  genres: string[]
  styles: string[]
}

export interface GenrePage {
  name: string
  total: number
  tracks: Track[]
  artists: ArtistSummary[]
  /** Genres linked to it on the genre cloud, strongest first (0 to 1). */
  related: { name: string; songs: number; strength: number }[]
}

/** Every genre in the library and how they relate (Library > Genres). */
export interface GenreMap {
  /** By name, with how many songs have each. */
  genres: { name: string; songs: number }[]
  /** Related genres, as indexes into `genres`; strength 0 to 1. */
  links: { a: number; b: number; strength: number }[]
}

/** Version and updates (Admin > About). */
export interface About {
  version: string
  commit: string
  commit_date: string
  repository: string
  contact: string
  check_updates: boolean
  auto_update: boolean
  /** Unix ms of the last check. */
  checked_at: number | null
  latest: Release | null
  update_available: boolean
  /** Why the update can't be installed from here. */
  cannot_install: string | null
  error: string | null
  installing: boolean
  /** [done, total] bytes while downloading an update. */
  progress: [number, number] | null
}

export interface Release {
  version: string
  url: string
  notes: string
  published_at: string | null
}

/** The AI reader for free-form search requests. */
export interface ReaderStatus {
  installed: boolean
  enabled: boolean
  ready: boolean
  download_size: number
  downloading: [number, number] | null
  error: string | null
  /** Built to use a GPU when there is one. */
  gpu_build: boolean
  /** Loaded onto a GPU. */
  on_gpu: boolean
  /** An NVIDIA graphics driver is installed, so the GPU pack can help. */
  nvidia: boolean
  gpu_installed: boolean
  /** Use the graphics card when the pack is installed. */
  gpu_enabled: boolean
  /** Rough download size of the pack, in bytes. */
  gpu_pack_size: number
  gpu_downloading: [number, number] | null
  gpu_error: string | null
  /** Why the reader runs on the processor although it could use the graphics card. */
  gpu_note: string | null
}

export interface IdentifyStatus {
  /** Songs missing something, not checked yet. */
  waiting: number
  /** Found, waiting for review. */
  found: number
  applied: number
  running: boolean
  needs_key: boolean
  needs_contact: boolean
}

export interface IdentificationRow {
  track_id: number
  score: number | null
  title: string | null
  artist: string | null
  album: string | null
  year: number | null
  genres: string | null
  current_title: string
  current_artist: string | null
  current_album: string | null
  current_year: number | null
}

/** Smart play's switches (Admin > Smart play). */
export interface SmartPlaySettings {
  smart_play: boolean
  smart_playlists: boolean
  /** How much what parties did counts, 0 to 100. */
  strength: number
}

/** A party style: a separate memory of what one kind of party liked. */
export interface PartyStyle {
  id: number
  name: string
  requests: number
  plays: number
  created_at: number
}

export interface SmartPlay extends SmartPlaySettings {
  /** The style in use. */
  active: number | null
  styles: PartyStyle[]
}

/** The most favored and most avoided, strongest first. */
export interface Extremes<T> {
  favored: T[]
  avoided: T[]
}

export interface LearnedSong {
  track: Track
  /** Above 1 favored, below 1 avoided. */
  weight: number
  requests: number
  finished: number
  skipped: number
}

export interface Learned {
  songs: Extremes<LearnedSong>
  artists: Extremes<{ artist: ArtistSummary; weight: number }>
  genres: Extremes<{ name: string; weight: number }>
}

/** Tags DJINN can correct; the music files aren't changed. */
export type TagField = 'title' | 'artist' | 'album' | 'album_artist' | 'year' | 'genre'

/** A song's corrections; null keeps the file's value. */
export type TagEdit = { [F in TagField]: F extends 'year' ? number | null : string | null }

export interface TrackEdit {
  track: Track
  edited: TagEdit
}

/** Changed fields: one left out stays, null goes back to the file's. */
export type TagFields = { [F in TagField]?: string | number | null }

/** Corrections for songs. */
export type TagChange = { track_ids: number[] } & TagFields

/** A backup waiting to be restored. */
export interface UploadedBackup {
  /** The DJINN that made it. */
  version: string
  created_ms: number
  /** Its music folders; `here` when the same folder exists on this computer. */
  folders: { from: string; here: boolean }[]
}

/** Admin > System: how DJINN is doing. */
export interface SystemStatus {
  version: string
  started_ms: number | null
  /** Where phones reach DJINN. */
  addresses: string[]
  /** Windows has the network set to Public, which blocks phones. */
  public_network: boolean
  songs: number
  library_folders: number
  scanning: boolean
  last_scan: { found: number; to_read: number; read: number; removed: number; done: boolean } | null
  state: PlayState
  outputs: OutputStatus
  casts: { name: string; playing: boolean; error: string | null }[]
  ai_listening: boolean
  ai_reader: boolean
  data_dir: string | null
  database_bytes: number
  art_bytes: number
  models_bytes: number
  log_file: string | null
}

/** What Admin > System can clear out. */
export type SystemReset = 'queue' | 'tag_edits' | 'analysis' | 'listening' | 'history' | 'settings' | 'everything'

/** A picture of an artist for slideshows. */
export interface ArtistPicture {
  art_hash: string
  /** "Author (License)". */
  credit: string
  /** Its page on Wikimedia Commons. */
  credit_url: string
  source: 'commons' | 'manual' | 'fanart' | 'audiodb' | 'unsplash'
  remote_url: string | null
}
