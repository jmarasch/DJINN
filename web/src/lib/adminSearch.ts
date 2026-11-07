// Every admin setting and tool, for searching the admin pages. `label` is
// the text on the page (search results find it there and highlight it),
// `key` the server's setting name when it has a default, for "changed from
// default".

import type { AdminTab } from './adminTabs'

export interface AdminEntry {
  label: string
  tab: AdminTab
  keywords?: string
  key?: string
}

export const adminEntries: AdminEntry[] = [
  // Rules
  { label: 'Guest mode', tab: 'rules', keywords: 'join name no account guests', key: 'guest_mode' },
  { label: 'TV page', tab: 'rules', keywords: 'tv tablet screen join code /tv', key: 'tv_page' },
  { label: 'Sound on the TV page', tab: 'rules', keywords: 'tv audio listen', key: 'tv_sound' },
  { label: 'Play here', tab: 'rules', keywords: 'tune in phone listen stream', key: 'play_here' },
  { label: 'Most songs in the queue at once', tab: 'rules', keywords: 'queue limit cap max', key: 'max_queue' },
  { label: 'Songs each person can have queued', tab: 'rules', keywords: 'requests limit per user', key: 'per_user_limit' },
  { label: 'Votes needed to skip a song', tab: 'rules', keywords: 'skip voting vote', key: 'skip_votes' },
  {
    label: 'Songs before a shuffled playlist song can repeat',
    tab: 'rules',
    keywords: 'history window repeat shuffle',
    key: 'history_window',
  },
  {
    label: "Don't play another copy of a song that just played",
    tab: 'rules',
    keywords: 'duplicates copies auto mix',
    key: 'avoid_copies',
  },
  // Smart play
  { label: 'Learn from the party for wild play and radio', tab: 'smart', keywords: 'smart play learning', key: 'smart_play' },
  { label: 'Also use it for shuffled playlists', tab: 'smart', keywords: 'smart playlists', key: 'smart_playlists' },
  { label: 'How much it counts', tab: 'smart', keywords: 'strength smart', key: 'smart_strength' },
  { label: 'Party styles', tab: 'smart', keywords: 'style learning forget' },
  // Audio
  { label: 'Crossfade', tab: 'audio', keywords: 'blend gapless fade transition', key: 'crossfade_secs' },
  { label: 'Outputs', tab: 'audio', keywords: 'speakers sound card devices delay volume drift' },
  { label: 'Cast to TVs and speakers', tab: 'audio', keywords: 'chromecast sonos dlna cast stream' },
  { label: 'Volume leveling (ReplayGain)', tab: 'audio', keywords: 'replaygain loudness volume boost' },
  { label: 'Equalizer', tab: 'audio', keywords: 'eq bass treble' },
  // Request log
  { label: 'Request log', tab: 'log', keywords: 'requests history who asked' },
  // Users
  { label: 'Accounts', tab: 'users', keywords: 'users people add password' },
  { label: 'Roles', tab: 'users', keywords: 'permissions listener downloads' },
  { label: 'Sign-ups', tab: 'users', keywords: 'create account approve register signup' },
  { label: 'Invite links', tab: 'users', keywords: 'invite link join' },
  { label: 'Waiting for approval', tab: 'users', keywords: 'pending approve' },
  { label: 'Guests', tab: 'users', keywords: 'guest list limit' },
  // GO
  { label: 'DJINN GO', tab: 'go', keywords: 'personal streaming go on off', key: 'go_enabled' },
  { label: 'Server name', tab: 'go', keywords: 'name' },
  { label: 'Let phones on this network find this server', tab: 'go', keywords: 'discover mdns', key: 'go_discoverable' },
  { label: 'Android app', tab: 'go', keywords: 'apk phone app qr' },
  { label: 'Songs kept on devices', tab: 'go', keywords: 'offline downloads kept sign out' },
  { label: 'Encrypt connections', tab: 'go', keywords: 'https tls certificate', key: 'go_encryption' },
  {
    label: 'Allow connections from outside the home network',
    tab: 'go',
    keywords: 'remote away internet',
    key: 'go_allow_remote',
  },
  {
    label: 'Also allow unencrypted connections from outside',
    tab: 'go',
    keywords: 'plain http remote',
    key: 'go_allow_plain_remote',
  },
  { label: 'Router port forwarding', tab: 'go', keywords: 'upnp router port 443', key: 'go_upnp' },
  { label: 'Public hostname', tab: 'go', keywords: 'domain https hostname' },
  { label: 'Admins must use two-step sign-in for GO', tab: 'go', keywords: '2fa totp security', key: 'admin_two_step' },
  { label: 'Trusted proxies', tab: 'go', keywords: 'reverse proxy cloudflare' },
  { label: 'Sign-in log', tab: 'go', keywords: 'logins security attempts' },
  { label: 'Converting', tab: 'go', keywords: 'convert mp3 transcode quality', key: 'convert_enabled' },
  { label: 'Submitted music', tab: 'go', keywords: 'uploads submissions approve', key: 'upload_max_mb' },
  // Library
  { label: 'Music folders', tab: 'library', keywords: 'folder add remove path drive' },
  { label: 'Check for changes', tab: 'library', keywords: 'scan rescan library check' },
  {
    label: 'Show all credited artists in the Artists library',
    tab: 'library',
    keywords: 'collaboration featured artists',
    key: 'show_collaboration_artists',
  },
  { label: 'Use the original release year', tab: 'library', keywords: 'year reissue date', key: 'original_years' },
  { label: 'Keep albums with the same name apart', tab: 'library', keywords: 'editions albums folders', key: 'album_folders' },
  {
    label: 'Songs and artists a genre page loads at a time',
    tab: 'library',
    keywords: 'genre page size',
    key: 'genre_page_size',
  },
  {
    label: "Hours before a genre's order is shuffled again",
    tab: 'library',
    keywords: 'genre refresh shuffle',
    key: 'genre_refresh_hours',
  },
  { label: 'Names that should never be split', tab: 'library', keywords: 'artist split ampersand feat' },
  { label: 'Sound analysis', tab: 'library', keywords: 'analyze more like this radio fingerprint', key: 'sound_analysis' },
  { label: 'When heavy work runs', tab: 'library', keywords: 'background hours idle schedule' },
  { label: 'Artist photos and bios', tab: 'library', keywords: 'pictures wikipedia deezer online', key: 'online_artist_info' },
  { label: 'More pictures, as a slideshow', tab: 'library', keywords: 'artist pictures slideshow', key: 'artist_pictures' },
  { label: 'Picture download limits', tab: 'library', keywords: 'image cache size pixels' },
  { label: 'Artist picture sources', tab: 'library', keywords: 'fanart audiodb unsplash commons api key' },
  { label: 'MusicBrainz', tab: 'library', keywords: 'covers genres lookup contact' },
  { label: 'Identify songs by sound', tab: 'library', keywords: 'acoustid fingerprint identify' },
  { label: 'YouTube playlist links', tab: 'library', keywords: 'youtube import api key' },
  { label: 'Image cache', tab: 'library', keywords: 'art covers folder' },
  // Problems
  { label: 'Problems', tab: 'problems', keywords: 'broken missing unsupported tags reported diagnostics' },
  { label: 'Possible duplicates', tab: 'problems', keywords: 'copies duplicate same song' },
  { label: 'Check the library every night', tab: 'problems', keywords: 'nightly scheduled check summary', key: 'scheduled_check' },
  // AI
  { label: 'Listen to songs with AI', tab: 'ai', keywords: 'ai listening clap genres' },
  {
    label: 'Give songs with no genres the ones the AI hears',
    tab: 'ai',
    keywords: 'ai genre tags',
    key: 'ai_genre_tags',
  },
  { label: 'Read searches with AI', tab: 'ai', keywords: 'ai reader qwen search' },
  { label: 'Use the graphics card', tab: 'ai', keywords: 'gpu nvidia cuda' },
  // System
  { label: 'Status', tab: 'system', keywords: 'running uptime addresses' },
  { label: 'Server ports', tab: 'system', keywords: 'port 8066 8067' },
  { label: 'Updates', tab: 'system', keywords: 'update version nightly stable channel', key: 'update_check' },
  { label: 'Install updates by themselves', tab: 'system', keywords: 'automatic updates', key: 'update_auto' },
  { label: 'Back up and restore', tab: 'system', keywords: 'backup restore zip' },
  { label: 'Log', tab: 'system', keywords: 'log errors tracing download' },
  { label: 'Clear and reset', tab: 'system', keywords: 'reset clear history start over' },
  // About
  { label: 'Credits', tab: 'about', keywords: 'about version license' },
]

const fold = (text: string) =>
  text
    .toLowerCase()
    .normalize('NFKD')
    .replace(/[^a-z0-9 ]/g, ' ')

/** Entries matching every word of `query`, best first. */
export function searchAdmin(query: string): AdminEntry[] {
  const words = fold(query).split(/\s+/).filter(Boolean)
  if (!words.length) return []
  return adminEntries
    .map((entry) => {
      const label = fold(entry.label)
      const all = `${label} ${fold(entry.keywords ?? '')} ${entry.tab}`
      if (!words.every((w) => all.includes(w))) return null
      const score = words.filter((w) => label.includes(w)).length * 2 + (label.startsWith(words[0]) ? 1 : 0)
      return { entry, score }
    })
    .filter((m): m is { entry: AdminEntry; score: number } => m !== null)
    .sort((a, b) => b.score - a.score)
    .map((m) => m.entry)
}
