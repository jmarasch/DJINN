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

export interface Track {
  id: number
  title: string
  artist: string | null
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
  other_tracks: Track[]
}

export interface QueueItem {
  id: number
  user_id: number
  username: string
  added_at: number
  track: Track
}

export type PlaySource = 'request' | 'pool'

export interface Entry {
  track: Track
  source: PlaySource
  requested_by: string | null
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
  /** Plays when nobody has requested anything. */
  playlist: PlaylistRef | null
}

export interface OutputStatus {
  device: string | null
  sample_rate: number | null
  error: string | null
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

export type TrackSort = 'title' | 'artist' | 'added'

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
