<script lang="ts">
  // Admin > Library: lyrics. Where they come from, the lyrics page, and the
  // export of .lrc files.
  import { onMount } from 'svelte'
  import { api } from './api'
  import { toast } from './state.svelte'
  import type { LyricsImportStatus, LyricsSettings, LyricsStatus } from './types'

  let settings = $state<LyricsSettings | null>(null)
  let status = $state<LyricsStatus | null>(null)
  let importing = $state<LyricsImportStatus | null>(null)
  let files = $state<FileList | null>(null)
  let replace = $state(false)
  let unasked = $state(false)
  let sending = $state(false)
  const n = (x: number) => x.toLocaleString()

  async function refresh() {
    try {
      status = await api.lyricsStatus()
      importing = await api.lyricsImport()
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
    // Keep it current: the job can start while this is open (after a
    // library check or a restart).
    const timer = setInterval(() => void refresh(), 5000)
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

  /** Sends the chosen zips one after another; DJINN imports one at a time. */
  async function sendZips() {
    if (!files?.length || sending) return
    sending = true
    try {
      for (const file of Array.from(files)) {
        await api.importLyrics(file, replace, unasked)
        // Wait for it to finish before the next.
        for (;;) {
          await new Promise((r) => setTimeout(r, 1500))
          await refresh()
          if (!importing?.running) break
        }
      }
      files = null
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not import', true)
    } finally {
      sending = false
      void refresh()
    }
  }

  async function dismissImport() {
    try {
      await api.dismissLyricsImport()
      importing = null
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not clear it', true)
    }
  }

  async function allowAgain() {
    if (!confirm('Let every removed lyric come back? Songs are read and asked about again.')) return
    try {
      await api.allowAllLyrics()
      toast('Done')
      setTimeout(refresh, 500)
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not do that', true)
    }
  }

  const report = $derived(importing?.report ?? null)
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
        )} instrumental{/if}{#if status.imported}; {n(status.imported)} imported from zips{/if}{#if status.ai} ({n(status.ai)} made by an AI){/if}.
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
  {#if status?.blocked}
    <p class="small">
      {n(status.blocked)} {status.blocked === 1 ? 'song has' : 'songs have'} lyrics you removed as bad, which won't be
      taken from the same source again unless its text changes.
      <button class="link" onclick={allowAgain}>Allow them again</button>
    </p>
  {/if}
  <p class="small muted">
    Recheck reads every song again (for .lrc files added since) and asks LRCLIB again. Export downloads a zip of .lrc
    files laid out like your music folders and named like the songs, so unzipping it over a music folder puts each one
    beside its song.
  </p>
</section>

<section class="card">
  <h3>Import lyrics from a zip</h3>
  <p class="small muted">
    A zip of .lrc or .txt files laid out like your music folders (the same layout as the export above). Each file is
    matched to a song by the artist and title in its [ar:] and [ti:] lines, or by its place in the zip when those are
    missing. It only fills songs that have no lyrics: lyrics from song files, tags and LRCLIB always win. Files that
    look like code or markup are thrown out and listed. Up to 250 MB and 150,000 files per zip; send several zips for a
    bigger library.
  </p>
  <label class="switch">
    <input type="checkbox" bind:checked={replace} />
    Replace lyrics from an earlier import
  </label>
  <label class="switch">
    <input type="checkbox" bind:checked={unasked} />
    Import even if LRCLIB has not been asked about the song yet
  </label>
  <p class="small muted sub">
    Unless this is ticked, songs LRCLIB hasn't been asked about are skipped (and counted), so it gets the first go.
  </p>
  <div class="actions">
    <input type="file" accept=".zip,application/zip" multiple bind:files disabled={sending || importing?.running} />
    <button onclick={sendZips} disabled={!files?.length || sending || importing?.running}>Import</button>
  </div>
  {#if report}
    <div class="report">
      <p class="small">
        {#if importing?.running}<span class="spinner" aria-hidden="true"></span>
          Importing {report.file}: {n(report.done)} of {n(report.total)} files.
        {:else}
          {report.file || 'The last import'}: {n(report.saved_timed + report.saved_plain)} saved ({n(report.saved_timed)}
          timed, {n(report.saved_plain)} plain).
        {/if}
      </p>
      {#if report.error}<p class="small bad">Stopped: {report.error}</p>{/if}
      {#if report.warning_count}
        <div class="warn">
          <p class="small">
            <strong>{n(report.warning_count)} {report.warning_count === 1 ? 'file was' : 'files were'} thrown out</strong>
            because {report.warning_count === 1 ? 'it looked' : 'they looked'} like code or markup, not lyrics:
          </p>
          <ul class="small">
            {#each report.warnings.slice(0, 20) as w (w.entry)}
              <li>{w.entry}: {w.reason}</li>
            {/each}
          </ul>
          {#if report.warning_count > 20}<p class="small muted">The rest are in the full report.</p>{/if}
        </div>
      {/if}
      <p class="small muted">
        Skipped: {n(report.had_lyrics)} already had lyrics, {n(report.kept_import)} kept an earlier import, {n(
          report.waiting_lrclib,
        )} waiting for LRCLIB, {n(report.instrumental + report.empty)} empty or instrumental. No song found for {n(
          report.not_matched,
        )}{#if report.ambiguous}; {n(report.ambiguous)} fit more than one song{/if}.
      </p>
      {#if !importing?.running}
        <div class="actions">
          <a class="button" href="/api/lyrics/import/report" download>Download the full report</a>
          <button onclick={dismissImport}>Clear</button>
        </div>
      {/if}
    </div>
  {/if}
</section>

<style>
  .report {
    margin-top: 12px;
    padding-top: 8px;
    border-top: 1px solid var(--line);
  }

  .warn {
    border: 1px solid var(--warn, #c80);
    border-radius: 8px;
    padding: 8px 12px;
    margin: 6px 0;
  }

  .warn ul {
    margin: 4px 0 0;
    padding-left: 18px;
  }

  .bad {
    color: var(--danger, #d33);
  }

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
