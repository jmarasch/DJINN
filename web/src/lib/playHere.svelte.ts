// Play here: the party on this phone or computer, from the same live
// stream casts play. It runs a few seconds behind the speakers.

import { api } from './api'
import { app, isAdmin, toast } from './state.svelte'

export const playHere = $state({
  /** The stream; null when the host turned Play here off. */
  url: null as string | null,
  /** Listening, or trying to. */
  on: false,
  /** Connecting, or waiting for sound after a blip. */
  waiting: false,
})

let audio: HTMLAudioElement | null = null
let retry: ReturnType<typeof setTimeout> | undefined

/** Asks whether Play here is allowed; stops listening if it no longer is. */
export async function checkPlayHere() {
  try {
    playHere.url = (await api.listen()).url
  } catch {
    playHere.url = null
  }
  if (!playHere.url && playHere.on) stopPlayHere()
}

function player(): HTMLAudioElement {
  if (audio) return audio
  const a = new Audio()
  a.preload = 'none'
  a.addEventListener('playing', () => (playHere.waiting = false))
  a.addEventListener('waiting', () => (playHere.waiting = true))
  // The server restarted or the network blipped: connect again.
  const reconnect = () => {
    if (!playHere.on) return
    playHere.waiting = true
    clearTimeout(retry)
    retry = setTimeout(async () => {
      await checkPlayHere()
      if (playHere.on) void connect()
    }, 3_000)
  }
  a.addEventListener('error', reconnect)
  a.addEventListener('ended', reconnect)
  // Paused from outside the app (headphones unplugged, another app's
  // sound): stop, so the next start is live rather than stale.
  a.addEventListener('pause', () => {
    if (playHere.on && a.src) stopPlayHere()
  })
  audio = a
  return a
}

async function connect() {
  if (!playHere.url) return
  const a = player()
  // A fresh connection each time, so it starts at the live point.
  a.src = `${playHere.url}?t=${Date.now()}`
  try {
    await a.play()
  } catch {
    stopPlayHere()
    toast("Couldn't play here. Check this device's sound and try again.", true)
  }
}

/** Starts listening. Browsers only allow it after a tap, so call it from one. */
export async function startPlayHere() {
  if (playHere.on || !playHere.url) return
  playHere.on = true
  playHere.waiting = true
  await connect()
}

export function stopPlayHere() {
  playHere.on = false
  playHere.waiting = false
  clearTimeout(retry)
  if (!audio) return
  audio.removeAttribute('src')
  audio.load()
}

// Lock screen and notification controls, showing the song playing.
if ('mediaSession' in navigator) {
  const session = navigator.mediaSession
  session.setActionHandler('play', () => void startPlayHere())
  session.setActionHandler('pause', stopPlayHere)
  session.setActionHandler('stop', stopPlayHere)
  $effect.root(() => {
    $effect(() => {
      const track = app.nowPlaying.current?.track
      if (!playHere.on || !track) {
        session.metadata = null
        return
      }
      session.metadata = new MediaMetadata({
        title: track.title,
        artist: track.artist ?? '',
        album: track.album ?? '',
        artwork: track.art_hash ? [{ src: api.artUrl(track.art_hash, 512), sizes: '512x512' }] : [],
      })
    })
    $effect(() => {
      // Admins can skip from the lock screen too.
      session.setActionHandler('nexttrack', playHere.on && isAdmin() ? () => void api.skip() : null)
    })
  })
}
