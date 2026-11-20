<script lang="ts">
  // Admin > GO: music people sent from DJINN GO, waiting for review, and
  // where approved songs go.
  import { onMount } from 'svelte'
  import { api } from './api'
  import { duration } from './format'
  import { unlocked } from './protected.svelte'
  import { toast } from './state.svelte'
  import { locale, t } from './i18n.svelte'
  import type { Submission, UploadSettings } from './types'

  let waiting = $state<Submission[]>([])
  let settings = $state<UploadSettings | null>(null)
  let busy = $state(false)

  onMount(() => {
    load().catch((e) => toast(e.message, true))
  })

  async function load() {
    ;[waiting, settings] = await Promise.all([api.waitingSubmissions(), api.uploadSettings()])
  }

  async function approve(s: Submission) {
    busy = true
    try {
      await unlocked(() => api.approveSubmission(s.id))
      waiting = waiting.filter((w) => w.id !== s.id)
      toast(t('sub.added', { title: s.title ?? s.file_name }))
    } catch (e) {
      toast(e instanceof Error ? e.message : t('common.add_failed'), true)
    } finally {
      busy = false
    }
  }

  async function decline(s: Submission) {
    const note = prompt(t('sub.decline_q', { title: s.title ?? s.file_name, who: s.uploader ?? t('sub.them') }), '')
    if (note === null) return
    try {
      await api.declineSubmission(s.id, note)
      waiting = waiting.filter((w) => w.id !== s.id)
    } catch (e) {
      toast(e instanceof Error ? e.message : t('sub.decline_failed'), true)
    }
  }

  async function saveSettings(e: SubmitEvent) {
    e.preventDefault()
    if (!settings) return
    try {
      settings = await api.saveUploadSettings(settings)
      toast(t('common.saved'))
    } catch (err) {
      toast(err instanceof Error ? err.message : t('common.save_failed_short'), true)
    }
  }

  const mb = (bytes: number) =>
    `${(bytes / 1024 / 1024).toLocaleString(locale.code, { minimumFractionDigits: 1, maximumFractionDigits: 1 })} MB`
</script>

<section class="card">
  <h2>{t('sub.title')}</h2>
  <p class="small muted">
    {t('sub.intro')}
  </p>
  {#each waiting as s (s.id)}
    <div class="item">
      <div>
        <b>{s.title ?? s.file_name}</b>
        <span class="muted">{[s.artist, s.album].filter(Boolean).join(' · ')}</span>
      </div>
      <div class="muted small">
        {t('sub.from', { who: s.uploader ?? t('sub.someone') })} · {s.file_name} · {mb(s.size)}{#if s.duration_ms} · {duration(s.duration_ms)}{/if}
      </div>
      {#if s.duplicate_title}<div class="warn small">{t('sub.dup', { title: s.duplicate_title })}</div>{/if}
      {#if !s.title || !s.artist}<div class="warn small">{!s.title ? t('sub.no_title') : t('sub.no_artist')}</div>{/if}
      <audio controls preload="none" src="/api/submissions/{s.id}/file"></audio>
      <div class="buttons">
        <button class="primary small-button" onclick={() => approve(s)} disabled={busy}>{t('sub.add')}</button>
        <button class="small-button" onclick={() => decline(s)} disabled={busy}>{t('sub.decline')}</button>
      </div>
    </div>
  {:else}
    <p class="muted">{t('sub.nothing')}</p>
  {/each}

  {#if settings}
    <form class="settings" onsubmit={saveSettings}>
      <label>
        {t('sub.folder')}
        <input bind:value={settings.folder} spellcheck="false" />
      </label>
      <p class="small muted">
        {t('sub.folder_note')}
      </p>
      <label>
        {t('sub.max')}
        <input type="number" min="1" max="4096" bind:value={settings.max_mb} />
      </label>
      <button class="small-button">{t('common.save')}</button>
    </form>
  {/if}
</section>

<style>
  .item {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 10px 0;
    border-bottom: 1px solid var(--line);
  }

  .warn {
    color: var(--bad);
  }

  audio {
    width: 100%;
  }

  .buttons {
    display: flex;
    gap: 8px;
  }

  .settings {
    display: flex;
    flex-direction: column;
    gap: 10px;
    margin-top: 8px;
  }

  .settings p {
    margin: 0;
  }

  .settings input[type='number'] {
    width: 120px;
  }

  .settings button {
    align-self: flex-start;
  }
</style>
