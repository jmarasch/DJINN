// Every admin setting and tool, for searching the admin pages. `label` is
// the text on the page (search results find it there and highlight it),
// `key` the server's setting name when it has a default, for "changed from
// default".

import type { AdminTab } from './adminTabs'
import { t } from './i18n.svelte'

export interface AdminEntry {
  /** English, for searching in English whatever the language. */
  label: string
  /** The language key of the setting's text on its page, which is what's
   * shown and highlighted. */
  text?: string
  tab: AdminTab
  keywords?: string
  key?: string
}

export const adminEntries: AdminEntry[] = [
  // Rules
  { label: 'Guest mode', text: 'admin.guest_mode', tab: 'rules', keywords: 'join name no account guests', key: 'guest_mode' },
  { label: 'TV page', text: 'rm.tv_page', tab: 'rules', keywords: 'tv tablet screen join code /tv', key: 'tv_page' },
  { label: 'Sound on the TV page', text: 'admin.tv_sound', tab: 'rules', keywords: 'tv audio listen', key: 'tv_sound' },
  { label: 'Play here', text: 'admin.play_here', tab: 'rules', keywords: 'tune in phone listen stream', key: 'play_here' },
  { label: 'Most songs in the queue at once', text: 'admin.max_queue', tab: 'rules', keywords: 'queue limit cap max', key: 'max_queue' },
  { label: 'Songs each person can have queued', text: 'admin.per_user', tab: 'rules', keywords: 'requests limit per user', key: 'per_user_limit' },
  { label: 'Votes needed to skip a song', text: 'rm.skip_votes', tab: 'rules', keywords: 'skip voting vote', key: 'skip_votes' },
  {
    label: 'Songs before a shuffled playlist song can repeat', text: 'admin.history_window',
    tab: 'rules',
    keywords: 'history window repeat shuffle',
    key: 'history_window',
  },
  {
    label: "Don't play another copy of a song that just played", text: 'admin.avoid_copies',
    tab: 'rules',
    keywords: 'duplicates copies auto mix',
    key: 'avoid_copies',
  },
  {
    label: 'Also list copies in different albums', text: 'dup.across',
    tab: 'library',
    keywords: 'duplicates copies songs albums greatest hits',
    key: 'duplicates_across_albums',
  },
  // Smart Play
  { label: 'Learn from the party for wild play and radio', text: 'sp.learn', tab: 'smart', keywords: 'smart play learning', key: 'smart_play' },
  { label: 'Also use it for shuffled playlists', text: 'sp.playlists', tab: 'smart', keywords: 'smart playlists', key: 'smart_playlists' },
  { label: 'How much it counts', text: 'sp.strength', tab: 'smart', keywords: 'strength smart', key: 'smart_strength' },
  { label: 'Show the mood dial', text: 'sp.mood_on', tab: 'smart', keywords: 'mood dial chill hype energy guests', key: 'mood_dial' },
  { label: 'How much the dial counts', text: 'sp.mood_strength', tab: 'smart', keywords: 'mood strength', key: 'mood_strength' },
  { label: 'Party schedule', text: 'admin.sub.schedule', tab: 'smart', keywords: 'schedule time rules quiet dance evening timer' },
  { label: 'Party styles', text: 'admin.sub.styles', tab: 'smart', keywords: 'style learning forget' },
  { label: 'Background', text: 'bgp.title', tab: 'system', keywords: 'wallpaper picture image theme look appearance' },
  // Audio
  { label: 'Crossfade', text: 'xf.title', tab: 'audio', keywords: 'blend gapless fade transition', key: 'crossfade_secs' },
  { label: 'Outputs', text: 'admin.sub.outputs', tab: 'audio', keywords: 'speakers sound card devices delay volume drift' },
  { label: 'Cast to TVs and speakers', text: 'cast.title', tab: 'audio', keywords: 'chromecast sonos dlna cast stream' },
  { label: 'Volume leveling (ReplayGain)', text: 'rg.title', tab: 'audio', keywords: 'replaygain loudness volume boost' },
  { label: 'Equalizer', text: 'eq.title', tab: 'audio', keywords: 'eq bass treble' },
  // Stats
  { label: 'Stats', text: 'admin.menu.stats', tab: 'stats', keywords: 'top songs artists requesters skipped busiest hours metrics recap' },
  { label: 'Clear this history', text: 'st.clear_title', tab: 'stats', keywords: 'delete plays requests forget' },
  { label: 'Searches', text: 'sw.title', tab: 'stats', keywords: 'search history nothing found shopping list' },
  { label: 'Keep a history of searches', text: 'sw.keep', tab: 'stats', keywords: 'search log privacy', key: 'search_history' },
  { label: 'Wishlist', text: 'sw.wishlist', tab: 'stats', keywords: 'wishes not in the library ask missing songs' },
  {
    label: 'Let everyone ask for songs that are not in the library', text: 'sw.everyone',
    tab: 'stats',
    keywords: 'wishlist guests',
    key: 'wishlist_everyone',
  },
  // Request Log
  { label: 'Request Log', text: 'admin.menu.log', tab: 'log', keywords: 'requests history who asked' },
  // Users
  { label: 'Accounts', text: 'up.accounts', tab: 'users', keywords: 'users people add password' },
  { label: 'Roles', text: 'up.roles', tab: 'users', keywords: 'permissions listener downloads' },
  { label: 'Sign-ups', text: 'up.signups', tab: 'users', keywords: 'create account approve register signup' },
  { label: 'Invite links', text: 'up.invites', tab: 'users', keywords: 'invite link join' },
  { label: 'Waiting for approval', text: 'up.waiting', tab: 'users', keywords: 'pending approve' },
  { label: 'Guests', text: 'up.guests', tab: 'users', keywords: 'guest list limit' },
  // GO
  { label: 'DJINN GO', text: 'admin.menu.go', tab: 'go', keywords: 'personal streaming go on off', key: 'go_enabled' },
  { label: 'Server name', text: 'gp.server_name', tab: 'go', keywords: 'name' },
  { label: 'Let phones on this network find this server', text: 'gp.discoverable', tab: 'go', keywords: 'discover mdns', key: 'go_discoverable' },
  { label: 'Android app', text: 'gp.android', tab: 'go', keywords: 'apk phone app qr' },
  { label: 'Songs kept on devices', text: 'gp.keep', tab: 'go', keywords: 'offline downloads kept sign out' },
  { label: 'Encrypt connections', text: 'gp.encrypt', tab: 'go', keywords: 'https tls certificate', key: 'go_encryption' },
  {
    label: 'Allow connections from outside the home network', text: 'gp.remote',
    tab: 'go',
    keywords: 'remote away internet',
    key: 'go_allow_remote',
  },
  {
    label: 'Also allow unencrypted connections from outside', text: 'gp.plain',
    tab: 'go',
    keywords: 'plain http remote',
    key: 'go_allow_plain_remote',
  },
  { label: 'Router port forwarding', text: 'gp.forwarding', tab: 'go', keywords: 'upnp router port 443', key: 'go_upnp' },
  { label: 'Public hostname', text: 'gp.hostname', tab: 'go', keywords: 'domain https hostname' },
  { label: 'Admins must use two-step sign-in for GO', text: 'gp.admin_two_step', tab: 'go', keywords: '2fa totp security', key: 'admin_two_step' },
  { label: 'Trusted proxies', text: 'gp.proxies', tab: 'go', keywords: 'reverse proxy cloudflare' },
  { label: 'Lyrics listening', text: 'aw.title', tab: 'ai', keywords: 'ai lyrics listener whisper worker timed sync install model local' },
  {
    label: 'Keep lyrics that score at least', text: 'aw.min',
    tab: 'ai',
    keywords: 'ai lyrics minimum score match confidence threshold slider',
    key: 'ai_lyrics_min_match',
  },
  { label: 'Genres to leave out', text: 'aw.skip', tab: 'ai', keywords: 'ai lyrics genres exclude skip instrumental classical' },
  { label: 'Library Editor', text: 'admin.menu.editor', tab: 'editor', keywords: 'compare database files tags differences corrections write grid' },
  { label: 'Protected Areas', text: 'admin.menu.protected', tab: 'protected', keywords: 'lock library changes strictness password two-step 2fa unlock max medium off' },
  { label: 'Audit trail', text: 'pa.audit', tab: 'protected', keywords: 'audit log records purge turn off who unlocked changed' },
  { label: 'Sign-In Log', text: 'admin.menu.signins', tab: 'signins', keywords: 'logins security attempts clear old' },
  { label: 'Converting', text: 'cv.title', tab: 'go', keywords: 'convert mp3 transcode quality', key: 'convert_enabled' },
  { label: 'Submitted music', text: 'sub.title', tab: 'go', keywords: 'uploads submissions approve', key: 'upload_max_mb' },
  // Library
  { label: 'Music folders', text: 'admin.home.folders', tab: 'library', keywords: 'folder add remove path drive' },
  { label: 'Check for changes', text: 'admin.check_changes', tab: 'library', keywords: 'scan rescan library check' },
  {
    label: 'Show all credited artists in the Artists library', text: 'admin.show_credited',
    tab: 'library',
    keywords: 'collaboration featured artists',
    key: 'show_collaboration_artists',
  },
  { label: 'Use the original release year', text: 'admin.original_years', tab: 'library', keywords: 'year reissue date', key: 'original_years' },
  { label: 'Keep albums with the same name apart', text: 'admin.album_folders', tab: 'library', keywords: 'editions albums folders', key: 'album_folders' },
  {
    label: 'Songs and artists a genre page loads at a time', text: 'admin.genre_page_size',
    tab: 'library',
    keywords: 'genre page size',
    key: 'genre_page_size',
  },
  {
    label: "Hours before a genre's order is shuffled again", text: 'admin.genre_refresh',
    tab: 'library',
    keywords: 'genre refresh shuffle',
    key: 'genre_refresh_hours',
  },
  {
    label: "How often Discover's shelves change", text: 'admin.discover_refresh',
    tab: 'library',
    keywords: 'discover shelves refresh recommendations suggestions',
    key: 'discover_refresh_minutes',
  },
  {
    label: 'A play counts once this much of the song is heard', text: 'admin.play_count',
    tab: 'library',
    keywords: 'play count percent plays heard',
    key: 'play_count_percent',
  },
  { label: 'Names that should never be split', text: 'admin.keep_whole', tab: 'library', keywords: 'artist split ampersand feat' },
  { label: 'Sound analysis', text: 'libx.sound_analysis', tab: 'library', keywords: 'analyze more like this radio fingerprint', key: 'sound_analysis' },
  { label: 'When heavy work runs', text: 'libx.heavy_when', tab: 'library', keywords: 'background hours idle schedule' },
  { label: 'Artist photos and bios', text: 'libx.artist_info', tab: 'library', keywords: 'pictures wikipedia deezer online', key: 'online_artist_info' },
  { label: 'Rooms', text: 'admin.menu.rooms', tab: 'rooms', keywords: 'room area zone link speakers party patio kitchen sync timing earlier later delay test sound' },
  { label: 'Default language', text: 'hl.default', tab: 'system', keywords: 'language translation spanish french german english idioma' },
  { label: 'Visualizers', text: 'admin.menu.visuals', tab: 'visuals', keywords: 'visualizer visuals spectrogram node screen rotation cycle' },
  { label: 'Allow nodes to ask to join', text: 'np.allow', tab: 'nodes', keywords: 'node pi raspberry room speaker box join pair', key: 'nodes_join' },
  { label: 'Look up missing lyrics on LRCLIB', text: 'lyrp.lookup', tab: 'library', keywords: 'lyrics words lrclib online synced', key: 'lyrics_lookup' },
  { label: 'Lyrics page', text: 'lyrp.page', tab: 'library', keywords: 'lyrics tv screen /lyrics karaoke', key: 'lyrics_page' },
  { label: 'Export .lrc files', text: 'lyrp.export', tab: 'library', keywords: 'lyrics export zip lrc download' },
  { label: 'Allow removed lyrics again', text: 'lyrp.allow', tab: 'library', keywords: 'lyrics removed blocked bad wrong report undo' },
  { label: 'Show a note on lyrics an AI worked on', text: 'lyrp.ai_note', tab: 'library', keywords: 'lyrics ai note made timed', key: 'lyrics_ai_note' },
  { label: 'Let better imported lyrics replace lyrics from files, tags and LRCLIB', text: 'lyrp.replace_other', tab: 'library', keywords: 'lyrics import replace timed ai synced' },
  { label: 'Import lyrics from a zip', text: 'lyrp.import_title', tab: 'library', keywords: 'lyrics import zip lrc txt bulk upload' },
  { label: 'Remove all imported lyrics', text: 'rs.lyrics', tab: 'system', keywords: 'lyrics imported zip bulk reset clear remove' },
  { label: 'Recheck lyrics', text: 'lyrp.recheck', tab: 'library', keywords: 'lyrics reread lrc' },
  { label: 'More pictures, as a slideshow', text: 'libx.more_pictures', tab: 'library', keywords: 'artist pictures slideshow', key: 'artist_pictures' },
  { label: 'Picture download limits', text: 'libx.picture_limits', tab: 'library', keywords: 'image cache size pixels' },
  { label: 'Artist picture sources', text: 'libx.sources', tab: 'library', keywords: 'fanart audiodb unsplash commons api key' },
  { label: 'MusicBrainz', tab: 'library', keywords: 'covers genres lookup contact' },
  {
    label: 'Show genres in their containers', text: 'gg.on',
    tab: 'genres',
    keywords: 'genre containers group similar genres auto metal combine',
    key: 'genre_containers',
  },
  { label: 'Genre map', text: 'gm.title', tab: 'genres', keywords: 'genre map drawing relations links influences web graph' },
  { label: 'Genre family tree', text: 'gw.title', tab: 'genres', keywords: 'genre tree family web map relations parent subgenre related' },
  { label: 'Identify songs by sound', text: 'libx.identify', tab: 'library', keywords: 'acoustid fingerprint identify' },
  { label: 'YouTube playlist links', text: 'libx.youtube', tab: 'library', keywords: 'youtube import api key' },
  { label: 'Image cache', text: 'admin.image_cache', tab: 'library', keywords: 'art covers folder' },
  // Problems
  { label: 'Problems', text: 'admin.home.problems', tab: 'problems', keywords: 'broken missing unsupported tags reported diagnostics' },
  { label: 'Possible duplicates', text: 'dup.title', tab: 'problems', keywords: 'copies duplicate same song' },
  { label: 'Check the library every night', text: 'sc.on', tab: 'problems', keywords: 'nightly scheduled check summary', key: 'scheduled_check' },
  // AI
  { label: 'Listen to songs with AI', text: 'ai.listen_on', tab: 'ai', keywords: 'ai listening clap genres' },
  {
    label: 'Give songs with no genres the ones the AI hears', text: 'ai.genre_tags',
    tab: 'ai',
    keywords: 'ai genre tags',
    key: 'ai_genre_tags',
  },
  { label: 'Read searches with AI', text: 'ai.reader_on', tab: 'ai', keywords: 'ai reader qwen search' },
  { label: 'Use the graphics card', text: 'ai.use_gpu', tab: 'ai', keywords: 'gpu nvidia cuda' },
  {
    label: 'Isolate the vocals here for nodes that help', text: 'aw.vocals_nodes',
    tab: 'ai',
    keywords: 'ai lyrics nodes vocals demucs',
    key: 'ai_vocals_for_nodes',
  },
  {
    label: "Give songs a node wasn't sure of to this computer's model", text: 'aw.second_opinion',
    tab: 'ai',
    keywords: 'ai lyrics nodes second opinion low score bigger model retry',
    key: 'ai_second_opinion',
  },
  { label: 'Spotify account for album lookups', text: 'libx.spotify', tab: 'library', keywords: 'spotify client id secret album lookup' },
  { label: 'Help with AI lyrics', text: 'nai.help', tab: 'nodes', keywords: 'ai lyrics node whisper helper' },
  // System
  { label: 'Status', text: 'ss.title', tab: 'home', keywords: 'running uptime addresses' },
  { label: 'Server ports', text: 'ss.ports', tab: 'home', keywords: 'port 8066 8067' },
  { label: 'Updates', text: 'admin.home.updates', tab: 'system', keywords: 'update version nightly stable channel', key: 'update_check' },
  { label: 'Install updates by themselves', text: 'upd.auto', tab: 'system', keywords: 'automatic updates', key: 'update_auto' },
  { label: 'Back up and restore', text: 'bk.title', tab: 'system', keywords: 'backup restore zip' },
  { label: 'Log', text: 'lv.title', tab: 'serverlog', keywords: 'log errors tracing download' },
  { label: 'Clear and reset', text: 'rs.title', tab: 'system', keywords: 'reset clear history start over' },
  // About
  { label: 'Credits', text: 'ab.credits', tab: 'home', keywords: 'about version license' },
]

const fold = (text: string) =>
  text
    .toLowerCase()
    .normalize('NFKD')
    .replace(/[\u0300-\u036f]/g, '')
    .replace(/ß/g, 'ss')
    .replace(/[^\p{L}\p{N} ]/gu, ' ')

/** An entry's name in the chosen language: its page text up to the first
 * colon, bracket or placeholder, which is also what's found on the page. */
export function entryLabel(entry: AdminEntry): string {
  if (!entry.text) return entry.label
  const full = t(entry.text)
  return full.split(/: |\s\(|\{|\. /)[0].trim() || entry.label
}

/** Entries matching every word of `query`, best first. */
export function searchAdmin(query: string): AdminEntry[] {
  const words = fold(query).split(/\s+/).filter(Boolean)
  if (!words.length) return []
  return adminEntries
    .map((entry) => {
      const label = fold(entryLabel(entry))
      const all = `${label} ${fold(entry.label)} ${fold(entry.keywords ?? '')} ${entry.tab}`
      if (!words.every((w) => all.includes(w))) return null
      const score = words.filter((w) => label.includes(w)).length * 2 + (label.startsWith(words[0]) ? 1 : 0)
      return { entry, score }
    })
    .filter((m): m is { entry: AdminEntry; score: number } => m !== null)
    .sort((a, b) => b.score - a.score)
    .map((m) => m.entry)
}
