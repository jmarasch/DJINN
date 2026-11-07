<script lang="ts">
  // Admin > Library: lyrics. Where they come from, the lyrics page, and the
  // export of .lrc files.
  import { onMount } from 'svelte'
  import { api } from './api'
  import { toast } from './state.svelte'
  import type { LyricsSettings, LyricsStatus } from './types'

  let settings = $state<LyricsSettings | null>(null)
  let status = $state<LyricsStatus | null>(null)
  const n = (x: number) => x.toLocaleString()

  async function refresh() {
    try {
      status = await api.lyricsStatus()
    } catch {
      // Shown next time.
    }
  }

  onMount(() => {
    api
      .lyricsSettings()
      .then((s) => (settings = s))
      .catch(() => {})
    void refresh()
    // While it works, show it moving.
    const timer = setInterval(() => {
      if (status?.running) void refresh()
    }, 3000)
    return () => clearInterval(timer)
  })

  async function save() {
    if (!settings) return
    try {
      await api.setLyricsSettings(settings)
      toast('Saved')
      setTimeout(refresh, 500)
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not save', true)
    }
  }

  async function recheck() {
    try {
      await api.recheckLyrics()
      toast('Looking for lyrics again')
      setTimeout(refresh, 500)
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not start', true)
    }
  }

  const withWords = $derived(status ? status.synced + status.plain : 0)
  const pageUrl = `${location.origin}/lyrics`
</script>

<section class="card">
  <h3>Lyrics</h3>
  <p class="small muted">
    DJINN reads lyrics from a .lrc file beside each song (same name) and from the songs' own tags, and keeps them in its
    database. Timed lyrics follow the music on the Playing tab, in GO and on the lyrics page; any lyrics can be searched
    by typing a line you remember.
  </p>
  {#if status}
    <p class="small">
      {#if status.running}<span class="spinner" aria-hidden="true"></span>
        {status.phase === 'looking up' ? 'Asking LRCLIB...' : `Reading files: ${n(status.unread)} to go.`}
      {/if}
      {n(withWords)} of {n(status.songs)} songs have lyrics ({n(status.synced)} timed){#if status.instrumental}, {n(
          status.instrumental,
        )} instrumental{/if}.
    </p>
  {/if}
  {#if settings}
    <label class="switch">
      <input type="checkbox" bind:checked={settings.lookup} onchange={save} />
      Look up missing lyrics on LRCLIB
    </label>
    <p class="small muted sub">
      A free, open lyrics site (lrclib.net). Each song's artist, title, album and length are sent, one song at a time.
      {#if settings.lookup && status && status.not_looked_up > 0}{n(status.not_looked_up)} songs still to ask about.{/if}
    </p>
    <label class="switch">
      <input type="checkbox" bind:checked={settings.page} onchange={save} />
      Lyrics page
    </label>
    <p class="small muted sub">
      Full-screen lyrics for a TV or spare screen, without signing in, at
      {#if settings.page}<a href="/lyrics" target="_blank" rel="noopener">{pageUrl}</a>{:else}{pageUrl}{/if}.
    </p>
  {/if}
  <div class="actions">
    <button onclick={recheck}>Recheck lyrics</button>
    {#if withWords}<a class="button" href="/api/lyrics/export" download>Export .lrc files</a>{/if}
  </div>
  <p class="small muted">
    Recheck reads every song again (for .lrc files added since) and asks LRCLIB again. Export downloads a zip of .lrc
    files laid out like your music folders and named like the songs, so unzipping it over a music folder puts each one
    beside its song.
  </p>
</section>

<style>
  .sub {
    margin: 2px 0 10px;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin-top: 6px;
  }
</style>
