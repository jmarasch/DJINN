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
}

export interface Mode {
  guest_mode: boolean
  setup_needed: boolean
  /** Name this device used last time, if recognized. */
  welcome_back: string | null
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

export interface ScanProgress {
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
  /** Empty turns MusicBrainz lookups off. */
  musicbrainz_contact: string
  /** Artist names never split, one per line. */
  artist_keep_whole: string
  /** The TV page (/tv) works without signing in. */
  tv_page: boolean
  /** Votes that skip a song; 0 turns skip voting off. */
  skip_votes: number
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

export type TrackSort = 'title' | 'artist' | 'added' | 'popular'

/** Year is newest first. */
export type AlbumSort = 'name' | 'year'

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

export interface LoudnessStatus {
  /** Songs whose loudness is known (tags or measured). */
  known: number
  /** Songs still to analyze. */
  waiting: number
  /** Songs analyzed (loudness and sound). */
  analyzed: number
  running: boolean
}

export interface ReplayGainSettings {
  mode: ReplayGainMode
  preamp_db: number
  max_preamp_db: number
  loudness: LoudnessStatus
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
}
