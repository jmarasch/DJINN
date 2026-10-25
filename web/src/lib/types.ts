// Mirrors the server's JSON.

export type Role = 'admin' | 'user' | 'guest'

export interface User {
  id: number
  username: string
  role: Role
  created_at: number
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
}

export interface Artist {
  id: number
  name: string
}

export interface SearchResults {
  tracks: Track[]
  albums: Album[]
  artists: Artist[]
}

export interface AlbumDetail extends Album {
  tracks: Track[]
}

export interface ArtistDetail extends Artist {
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

export interface PlayerState {
  state: PlayState
  volume: number
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

export type PoolKind = 'track' | 'album' | 'artist' | 'playlist'

export interface PoolEntry {
  id: number
  kind: PoolKind
  ref_id: number
  name: string | null
  added_at: number
}

export interface Settings {
  history_window: number
  per_user_limit: number
}
