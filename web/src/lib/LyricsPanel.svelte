<script lang="ts">
  // Admin > Library: lyrics. Where they come from, the lyrics page, and the
  // export of .lrc files.
  import { onMount } from 'svelte'
  import { api } from './api'
  import { toast } from './state.svelte'
  import { locale, t } from './i18n.svelte'
  import type { LyricsImportStatus, LyricsSettings, LyricsStatus } from './types'

  let settings = $state<LyricsSettings | null>(null)
  let status = $state<LyricsStatus | null>(null)
  let importing = $state<LyricsImportStatus | null>(null)
  let files = $state<FileList | null>(null)
  let replace = $state(false)
  let unasked = $state(false)
  let replaceOther = $state(false)
  let sending = $state(false)
  const n = (x: number) => x.toLocaleString(locale.code)

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
      toast(t('common.saved'))
      setTimeout(refresh, 500)
    } catch (e) {
      toast(e instanceof Error ? e.message : t('common.save_failed_short'), true)
    }
  }

  async function recheck() {
    try {
      await api.recheckLyrics()
      toast(t('lyrp.recheck_started'))
      setTimeout(refresh, 500)
    } catch (e) {
      toast(e instanceof Error ? e.message : t('common.start_failed'), true)
    }
  }

  /** Sends the chosen zips one after another; DJINN imports one at a time. */
  async function sendZips() {
    if (!files?.length || sending) return
    sending = true
    try {
      for (const file of Array.from(files)) {
        await api.importLyrics(file, replace, unasked, replaceOther)
        // Wait for it to finish before the next.
        for (;;) {
          await new Promise((r) => setTimeout(r, 1500))
          await refresh()
          if (!importing?.running) break
        }
      }
      files = null
    } catch (e) {
      toast(e instanceof Error ? e.message : t('lyrp.import_failed'), true)
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
      toast(e instanceof Error ? e.message : t('lyrp.clear_failed'), true)
    }
  }

  async function allowAgain() {
    if (!confirm(t('lyrp.allow_q'))) return
    try {
      await api.allowAllLyrics()
      toast(t('common.done'))
      setTimeout(refresh, 500)
    } catch (e) {
      toast(e instanceof Error ? e.message : t('common.do_failed'), true)
    }
  }

  const report = $derived(importing?.report ?? null)
  const withWords = $derived(status ? status.synced + status.plain : 0)
  const pageUrl = `${location.origin}/lyrics`
</script>

<section class="card">
  <h3>{t('lyrp.title')}</h3>
  <p class="small muted">
    {t('lyrp.intro')}
  </p>
  {#if status}
    <p class="small">
      {#if status.running}<span class="spinner" aria-hidden="true"></span>
        {status.phase === 'looking up' ? t('lyrp.asking') : t('lyrp.reading', { n: n(status.unread) })}
      {/if}
      {t('lyrp.have', { words: n(withWords), songs: n(status.songs), timed: n(status.synced) })}{#if status.instrumental}{t(
          'lyrp.instrumental',
          { n: n(status.instrumental) },
        )}{/if}{#if status.imported}{t('lyrp.imported', { n: n(status.imported) })}{/if}{#if status.ai}{t('lyrp.by_ai', {
            n: n(status.ai),
          })}{/if}.
    </p>
  {/if}
  {#if settings}
    <label class="switch">
      <input type="checkbox" bind:checked={settings.lookup} onchange={save} />
      {t('lyrp.lookup')}
    </label>
    <p class="small muted sub">
      {t('lyrp.lookup_note')}
      {#if settings.lookup && status && status.not_looked_up > 0}{t('lyrp.to_ask', { n: n(status.not_looked_up) })}{/if}
    </p>
    <label class="switch">
      <input type="checkbox" bind:checked={settings.ai_note} onchange={save} />
      {t('lyrp.ai_note')}
    </label>
    <p class="small muted sub">
      {t('lyrp.ai_note_note')}
    </p>
    <label class="switch">
      <input type="checkbox" bind:checked={settings.page} onchange={save} />
      {t('lyrp.page')}
    </label>
    <p class="small muted sub">
      {t('lyrp.page_note')}
      {#if settings.page}<a href="/lyrics" target="_blank" rel="noopener">{pageUrl}</a>{:else}{pageUrl}{/if}.
    </p>
  {/if}
  <div class="actions">
    <button onclick={recheck}>{t('lyrp.recheck')}</button>
    {#if withWords}<a class="button" href="/api/lyrics/export" download>{t('lyrp.export')}</a>{/if}
  </div>
  {#if status?.blocked}
    <p class="small">
      {t('lyrp.blocked', { n: status.blocked })}
      <button class="link" onclick={allowAgain}>{t('lyrp.allow')}</button>
    </p>
  {/if}
  <p class="small muted">
    {t('lyrp.recheck_note')}
  </p>
</section>

<section class="card">
  <h3>{t('lyrp.import_title')}</h3>
  <p class="small muted">
    {t('lyrp.import_note')}
  </p>
  <label class="switch">
    <input type="checkbox" bind:checked={replace} />
    {t('lyrp.replace')}
  </label>
  <label class="switch">
    <input type="checkbox" bind:checked={replaceOther} />
    {t('lyrp.replace_other')}
  </label>
  <p class="small muted sub">
    {t('lyrp.better_note')}
  </p>
  <label class="switch">
    <input type="checkbox" bind:checked={unasked} />
    {t('lyrp.unasked')}
  </label>
  <p class="small muted sub">
    {t('lyrp.unasked_note')}
  </p>
  <div class="actions">
    <input type="file" accept=".zip,application/zip" multiple bind:files disabled={sending || importing?.running} />
    <button onclick={sendZips} disabled={!files?.length || sending || importing?.running}>{t('lyrp.import')}</button>
  </div>
  {#if report}
    <div class="report">
      <p class="small">
        {#if importing?.running}<span class="spinner" aria-hidden="true"></span>
          {t('lyrp.importing', { file: report.file, done: n(report.done), total: n(report.total) })}
        {:else}
          {t('lyrp.saved', {
            file: report.file || t('lyrp.last_import'),
            saved: n(report.saved_timed + report.saved_plain),
            timed: n(report.saved_timed),
            plain: n(report.saved_plain),
          })}
        {/if}
      </p>
      {#if report.error}<p class="small bad">{t('lyrp.stopped', { error: report.error })}</p>{/if}
      {#if report.warning_count}
        <div class="warn">
          <p class="small">
            <strong>{t('lyrp.thrown_out', { n: report.warning_count })}</strong>
            {t('lyrp.because', { n: report.warning_count })}
          </p>
          <ul class="small">
            {#each report.warnings.slice(0, 20) as w (w.entry)}
              <li>{w.entry}: {w.reason}</li>
            {/each}
          </ul>
          {#if report.warning_count > 20}<p class="small muted">{t('lyrp.rest')}</p>{/if}
        </div>
      {/if}
      <p class="small muted">
        {t('lyrp.skipped', {
          had: n(report.had_lyrics),
          kept: n(report.kept_import),
          waiting: n(report.waiting_lrclib),
          empty: n(report.instrumental + report.empty),
          unmatched: n(report.not_matched),
        })}{#if report.ambiguous}{t('lyrp.ambiguous', { n: n(report.ambiguous) })}{/if}.
      </p>
      {#if !importing?.running}
        <div class="actions">
          <a class="button" href="/api/lyrics/import/report" download>{t('lyrp.download_report')}</a>
          <button onclick={dismissImport}>{t('common.clear')}</button>
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
