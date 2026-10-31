// DJINN GO's player. Songs come as their original files and this device
// plays them. Two audio elements take turns: while one plays, the next
// song loads in the other, so albums run on with hardly a gap.
//
// Songs you add (Add to queue, Play next, or a tap when taps add) wait
// right after the song playing, ahead of the rest of the queue: the album,
// playlist or radio carries on after them, the way requests go before the
// party's auto mix.

import { SvelteSet } from 'svelte/reactivity'
import { api } from '../lib/api'
import { toast } from '../lib/state.svelte'
import type { GoPrefs, LikeKind, PlayLinks, Track } from '../lib/types'

export type Repeat = 'off' | 'all' | 'one'

export const player = $state({
  /** In play order (shuffled when shuffle is on). */
  queue: [] as Track[],
  index: -1,
  playing: false,
  /** Seconds. */
  position: 0,
  duration: 0,
  shuffle: false,
  repeat: 'off' as Repeat,
  /** The queue is personal radio: more songs come as it runs low. */
  radio: false,
  downloadUrl: null as string | null,
  loading: false,
  /** Converting is on for this account (learned from play links). */
  canConvert: false,
  /** This device asks for converted MP3s to use less data. */
  saveData: readSaveData(),
})

function readSaveData(): boolean {
  try {
    return localStorage.getItem('go-save-data') === '1'
  } catch {
    return false
  }
}

export function setSaveData(on: boolean) {
  player.saveData = on
  try {
    localStorage.setItem('go-save-data', on ? '1' : '0')
  } catch {
    // Private browsing: it lasts until the page closes.
  }
  preloaded = null
  preloadNext()
}

/** Links for a song: converted when saving data, or when this device
 * can't play the original and converting is available. */
async function linksFor(trackId: number): Promise<PlayLinks> {
  const links = await api.playLinks(trackId, player.saveData)
  player.canConvert = links.can_convert
  if (!links.converted && links.can_convert && !canPlay(links.mime)) {
    return api.playLinks(trackId, true)
  }
  return links
}

/** What you like, for the hearts. */
export const likes = {
  track: new SvelteSet<number>(),
  album: new SvelteSet<number>(),
  artist: new SvelteSet<number>(),
}

export async function loadLikes() {
  const ids = await api.goLikedIds()
  likes.track.clear()
  likes.album.clear()
  likes.artist.clear()
  ids.tracks.forEach((id) => likes.track.add(id))
  ids.albums.forEach((id) => likes.album.add(id))
  ids.artists.forEach((id) => likes.artist.add(id))
}

export async function toggleLike(kind: LikeKind, id: number) {
  const set = likes[kind]
  const on = !set.has(id)
  if (on) set.add(id)
  else set.delete(id)
  try {
    await api.goLike(kind, id, on)
  } catch (e) {
    if (on) set.delete(id)
    else set.add(id)
    toast(e instanceof Error ? e.message : 'Could not save that', true)
  }
}

export const current = () => (player.index >= 0 ? player.queue[player.index] ?? null : null)

const decks = [new Audio(), new Audio()]
let active = 0
/** The next song, already loading in the other deck. */
let preloaded: { trackId: number; links: PlayLinks } | null = null
/** Play order before shuffling, to put back (without songs you added). */
let unshuffled: Track[] = []
/** Songs you added that haven't played yet. Each is its own copy, so the
 * same song elsewhere in the queue isn't mistaken for it. */
const added = new SvelteSet<Track>()

/** You added this song and it hasn't played yet. */
export const isAdded = (track: Track) => added.has(track)

/** Where the next song you add goes: after the song playing and the ones
 * you added before. */
function addSpot(): number {
  let at = player.index + 1
  while (at < player.queue.length && added.has(player.queue[at])) at++
  return at
}

/** Your GO preferences, from your account. */
export const prefs = $state<GoPrefs>({ tap: 'play' })

export async function loadPrefs() {
  Object.assign(prefs, await api.goPrefs())
}

export async function setTap(tap: GoPrefs['tap']) {
  const before = prefs.tap
  prefs.tap = tap
  try {
    await api.setGoPrefs({ ...prefs })
  } catch (e) {
    prefs.tap = before
    toast(e instanceof Error ? e.message : 'Could not save that', true)
  }
}
/** How far the current song got, for history. */
let heardMs = 0
let lastSave = 0
/** Seconds to seek to once the current song loads (carrying on). */
let startAt = 0
/** Ignores events from a deck after it's been swapped out. */
let generation = 0

for (const deck of decks) {
  deck.preload = 'auto'
  deck.addEventListener('timeupdate', () => {
    if (deck !== decks[active]) return
    player.position = deck.currentTime
    heardMs = Math.max(heardMs, deck.currentTime * 1000)
    if (player.playing && Date.now() - lastSave > 15_000) saveResume()
    updatePositionState()
  })
  deck.addEventListener('durationchange', () => {
    if (deck === decks[active]) player.duration = Number.isFinite(deck.duration) ? deck.duration : 0
  })
  deck.addEventListener('loadedmetadata', () => {
    if (deck === decks[active] && startAt > 0) {
      deck.currentTime = startAt
      startAt = 0
    }
  })
  deck.addEventListener('play', () => {
    if (deck === decks[active]) player.playing = true
  })
  deck.addEventListener('pause', () => {
    if (deck !== decks[active]) return
    player.playing = false
    saveResume()
  })
  deck.addEventListener('ended', () => {
    if (deck === decks[active]) songEnded()
  })
  deck.addEventListener('error', () => {
    if (deck !== decks[active] || !deck.src) return
    const song = current()
    if (song) toast(`Couldn't play "${song.title}"`, true)
    player.playing = false
  })
}

const canPlay = (mime: string) => decks[0].canPlayType(mime) !== ''

/** Records the song being left in your history. */
function report() {
  const song = current()
  if (song && heardMs > 0) api.goPlayed(song.id, heardMs).catch(() => {})
  heardMs = 0
}

/** Starts the song at `index`. Songs this device can't play are skipped. */
async function start(index: number, autoplay = true) {
  const song = player.queue[index]
  if (!song) return
  added.delete(song)
  const mine = ++generation
  player.loading = true
  try {
    let links: PlayLinks
    if (preloaded?.trackId === song.id) {
      links = preloaded.links
      // The other deck has it loaded already: switch to it.
      decks[active].pause()
      active = 1 - active
    } else {
      links = await linksFor(song.id)
      if (mine !== generation) return
      decks[active].pause()
      decks[active].src = links.url
    }
    preloaded = null
    if (!canPlay(links.mime)) {
      toast(`This device can't play "${song.title}" (${links.mime}), so it was skipped`, true)
      player.index = index
      return skipAhead()
    }
    const deck = decks[active]
    deck.volume = Math.min(1, Math.max(0, links.gain))
    player.index = index
    player.position = 0
    player.duration = song.duration_ms ? song.duration_ms / 1000 : 0
    player.downloadUrl = links.download_url
    heardMs = 0
    setMediaSession(song)
    if (autoplay) await deck.play().catch(() => (player.playing = false))
    saveResume()
    preloadNext()
    if (player.radio && player.queue.length - index < 5) extendRadio()
  } catch (e) {
    toast(e instanceof Error ? e.message : 'Could not play that song', true)
  } finally {
    if (mine === generation) player.loading = false
  }
}

/** Where the queue goes after `index`, or -1. */
function after(index: number): number {
  if (player.repeat === 'one') return index
  if (index + 1 < player.queue.length) return index + 1
  return player.repeat === 'all' && player.queue.length ? 0 : -1
}

async function preloadNext() {
  const at = after(player.index)
  const song = player.queue[at]
  if (!song || at === player.index) return
  try {
    const links = await linksFor(song.id)
    if (player.queue[after(player.index)]?.id !== song.id || !canPlay(links.mime)) return
    const other = decks[1 - active]
    other.src = links.url
    other.load()
    preloaded = { trackId: song.id, links }
  } catch {
    preloaded = null
  }
}

function songEnded() {
  heardMs = Math.max(heardMs, player.duration * 1000)
  report()
  const at = after(player.index)
  if (at < 0) {
    player.playing = false
    saveResume()
    return
  }
  if (at === player.index) {
    decks[active].currentTime = 0
    decks[active].play().catch(() => {})
    return
  }
  start(at)
}

function skipAhead() {
  const at = after(player.index)
  if (at >= 0 && at !== player.index) start(at)
  else player.playing = false
}

/** Plays `tracks` from `from`, replacing the queue. */
export function playList(tracks: Track[], from = 0, options: { radio?: boolean } = {}) {
  if (!tracks.length) return
  report()
  preloaded = null
  added.clear()
  player.radio = options.radio ?? false
  unshuffled = [...tracks]
  if (player.shuffle) {
    const first = tracks[from]
    const rest = shuffled(tracks.filter((_, i) => i !== from))
    player.queue = [first, ...rest]
    start(0)
  } else {
    player.queue = [...tracks]
    start(from)
  }
}

/** Nothing is loaded, or the queue played to its end. */
const idle = () => player.index < 0 || (!player.playing && decks[active].ended)

/** Puts copies of `tracks` in the queue at `at`, as songs you added. */
function insertAdded(tracks: Track[], at: number) {
  player.queue.splice(at, 0, ...tracks.map((t) => ({ ...t })))
  // The queue holds its own (reactive) copies: remember those.
  for (let i = at; i < at + tracks.length; i++) added.add(player.queue[i])
  preloaded = null
  preloadNext()
}

/** Right after the song playing, ahead of everything else. */
export function playNext(track: Track) {
  if (idle()) return playList([track])
  insertAdded([track], player.index + 1)
  toast(`"${track.title}" plays next`)
}

/** After the song playing and what you added before, ahead of the rest of
 * the album, playlist or radio. */
export function addToQueue(tracks: Track[]) {
  if (idle()) return playList(tracks)
  const ahead = addSpot() - player.index - 1
  insertAdded(tracks, addSpot())
  const what = tracks.length === 1 ? `"${tracks[0].title}"` : `${tracks.length} songs`
  toast(ahead ? `Added ${what}; plays after the ${ahead} you added before` : `Added ${what}; plays next`)
}

/** A tap on a song: plays it now, or adds it to the queue, as chosen on
 * your account page. With nothing playing, it plays. */
export function tapSong(track: Track, playNow: () => void) {
  if (prefs.tap === 'queue' && !idle()) addToQueue([track])
  else playNow()
}

export function jumpTo(index: number) {
  report()
  start(index)
}

export function removeAt(index: number) {
  if (index === player.index) return
  const [gone] = player.queue.splice(index, 1)
  added.delete(gone)
  if (index < player.index) player.index--
  const at = unshuffled.findIndex((t) => t.id === gone.id)
  if (at >= 0) unshuffled.splice(at, 1)
  preloaded = null
  preloadNext()
}

export function toggle() {
  const deck = decks[active]
  if (player.index < 0) return
  if (deck.paused) deck.play().catch(() => {})
  else deck.pause()
}

export function next() {
  if (player.index < 0) return
  report()
  const at = player.index + 1 < player.queue.length ? player.index + 1 : player.repeat === 'all' ? 0 : -1
  if (at >= 0) start(at)
}

/** Back to the start of the song, or the one before when near the start. */
export function previous() {
  if (player.index < 0) return
  if (decks[active].currentTime > 3 || player.index === 0) {
    decks[active].currentTime = 0
    return
  }
  report()
  start(player.index - 1)
}

export function seek(seconds: number) {
  const deck = decks[active]
  if (Number.isFinite(seconds)) deck.currentTime = Math.max(0, seconds)
}

export function setShuffle(on: boolean) {
  player.shuffle = on
  const song = current()
  if (!song) return
  // Songs you added stay right after the song playing either way.
  const picks = player.queue.slice(player.index + 1).filter((t) => added.has(t))
  if (on) {
    const rest = shuffled(player.queue.filter((t, i) => i !== player.index && !added.has(t)))
    player.queue = [song, ...picks, ...rest]
    player.index = 0
  } else {
    let at = unshuffled.indexOf(song)
    if (at < 0) at = unshuffled.findIndex((t) => t.id === song.id)
    player.queue =
      at < 0 ? [song, ...picks, ...unshuffled] : [...unshuffled.slice(0, at + 1), ...picks, ...unshuffled.slice(at + 1)]
    player.index = Math.max(0, at)
  }
  preloaded = null
  preloadNext()
}

export function cycleRepeat() {
  player.repeat = player.repeat === 'off' ? 'all' : player.repeat === 'all' ? 'one' : 'off'
  preloaded = null
  preloadNext()
}

/** Radio from a song: it and songs like it. The song playing carries on,
 * and so do songs you added; the radio takes over after them. */
export async function startRadio(track: Track) {
  try {
    const songs = await api.goRadio(track.id)
    if (idle()) {
      playList(songs, 0, { radio: true })
      toast(`Radio from "${track.title}"`)
      return
    }
    const keep = player.queue.slice(0, addSpot())
    // From the song playing: it isn't played twice.
    if (songs[0]?.id === current()?.id) songs.shift()
    player.queue = [...keep, ...songs]
    unshuffled = [...keep.filter((t) => !added.has(t)), ...songs]
    player.radio = true
    preloaded = null
    preloadNext()
    toast(`Radio from "${track.title}" comes after this song`)
  } catch (e) {
    toast(e instanceof Error ? e.message : 'Could not start radio', true)
  }
}

/** Plays a song now, then songs like it. */
export async function playRadio(track: Track) {
  try {
    playList(await api.goRadio(track.id), 0, { radio: true })
  } catch (e) {
    toast(e instanceof Error ? e.message : 'Could not play that song', true)
  }
}

let extending = false
async function extendRadio() {
  const last = player.queue[player.queue.length - 1]
  if (!last || extending) return
  extending = true
  try {
    const known = new Set(player.queue.map((t) => t.id))
    const more = (await api.goRadio(last.id)).filter((t) => !known.has(t.id))
    player.queue.push(...more)
    unshuffled.push(...more)
  } catch {
    // Radio just ends.
  } finally {
    extending = false
  }
}

/** Carries on where you left off, here or on another device. */
export function carryOn(songs: Track[], order: number[], position: number, positionMs: number) {
  const byId = new Map(songs.map((s) => [s.id, s]))
  const queue = order.map((id) => byId.get(id)).filter((t): t is Track => !!t)
  if (!queue.length) return
  report()
  preloaded = null
  added.clear()
  player.radio = false
  player.shuffle = false
  unshuffled = [...queue]
  player.queue = queue
  startAt = positionMs / 1000
  start(Math.min(Math.max(0, position), queue.length - 1))
}

function saveResume() {
  if (player.index < 0) return
  lastSave = Date.now()
  api
    .goSaveResume(
      player.queue.map((t) => t.id),
      player.index,
      decks[active].currentTime * 1000,
    )
    .catch(() => {})
}

/** Stops everything, for signing out. */
export function stop() {
  report()
  for (const deck of decks) {
    deck.pause()
    deck.removeAttribute('src')
  }
  preloaded = null
  player.queue = []
  player.index = -1
  player.playing = false
}

function shuffled<T>(items: T[]): T[] {
  const out = [...items]
  for (let i = out.length - 1; i > 0; i--) {
    const j = Math.floor(Math.random() * (i + 1))
    ;[out[i], out[j]] = [out[j], out[i]]
  }
  return out
}

function setMediaSession(song: Track) {
  if (!('mediaSession' in navigator)) return
  navigator.mediaSession.metadata = new MediaMetadata({
    title: song.title,
    artist: song.artist ?? '',
    album: song.album ?? '',
    artwork: song.art_hash ? [{ src: `/api/art/${song.art_hash}?size=512`, sizes: '512x512' }] : [],
  })
}

function updatePositionState() {
  if (!('mediaSession' in navigator) || !player.duration) return
  try {
    navigator.mediaSession.setPositionState({
      duration: player.duration,
      position: Math.min(player.position, player.duration),
      playbackRate: 1,
    })
  } catch {
    // Some browsers reject a position past the end while loading.
  }
}

if ('mediaSession' in navigator) {
  const handlers: [MediaSessionAction, MediaSessionActionHandler][] = [
    ['play', () => toggle()],
    ['pause', () => toggle()],
    ['nexttrack', () => next()],
    ['previoustrack', () => previous()],
    ['seekto', (d) => d.seekTime !== undefined && seek(d.seekTime)],
  ]
  for (const [action, handler] of handlers) {
    try {
      navigator.mediaSession.setActionHandler(action, handler)
    } catch {
      // Not every browser knows every action.
    }
  }
}

// Leaving the page counts as where you got to.
window.addEventListener('pagehide', () => {
  report()
  saveResume()
})
