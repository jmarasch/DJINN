<script lang="ts">
  // Admin > System: checking for, and installing, new versions.
  import { api } from './api'
  import { toast } from './state.svelte'
  import { locale, t } from './i18n.svelte'
  import type { About } from './types'

  let about = $state<About | null>(null)
  let checking = $state(false)
  /** The update is installed and DJINN is starting again. */
  let restarting = $state(false)

  $effect(() => {
    api
      .about()
      .then((a) => (about = a))
      .catch((e) => toast(e.message, true))
  })

  // While an update downloads, follow along; once DJINN goes away to
  // restart, wait for it and reload into the new version.
  $effect(() => {
    if (!about?.installing && !restarting) return
    const started = about?.version
    const timer = setInterval(async () => {
      try {
        const now = await api.about()
        if (restarting && now.version !== started) location.reload()
        about = now
      } catch {
        restarting = true
      }
    }, 1500)
    return () => clearInterval(timer)
  })

  async function check() {
    checking = true
    try {
      about = await api.checkForUpdate()
    } catch (e) {
      toast(e instanceof Error ? e.message : t('upd.check_failed'), true)
    } finally {
      checking = false
    }
  }

  async function install() {
    if (!about?.latest) return
    if (!confirm(t('upd.install_q', { version: about.latest.version }))) return
    try {
      await api.installUpdate()
      about = await api.about()
    } catch (e) {
      toast(e instanceof Error ? e.message : t('upd.install_failed'), true)
    }
  }

  async function setUpdates(change: Partial<Pick<About, 'check_updates' | 'auto_update' | 'nightly'>>) {
    if (!about) return
    const next = {
      check_updates: about.check_updates,
      auto_update: about.auto_update,
      nightly: about.nightly,
      ...change,
    }
    try {
      await api.setUpdateSettings(next)
      // Changing the channel checks again, so fetch what it found.
      about = 'nightly' in change ? await api.about() : { ...about, ...next }
    } catch (e) {
      toast(e instanceof Error ? e.message : t('common.failed'), true)
    }
  }

  const mb = (bytes: number) =>
    `${(bytes / 1_000_000).toLocaleString(locale.code, { minimumFractionDigits: 1, maximumFractionDigits: 1 })} MB`
  function ago(ms: number): string {
    const minutes = Math.round((Date.now() - ms) / 60_000)
    if (minutes < 1) return t('ni.just_now')
    if (minutes < 60) return t('ni.min_ago', { n: minutes })
    const hours = Math.round(minutes / 60)
    if (hours < 24) return t('ni.h_ago', { n: hours })
    return new Date(ms).toLocaleDateString(locale.code)
  }
</script>

{#if about}
  <section class="card">
    <h2>{t('upd.title')}</h2>
    {#if restarting}
      <p class="small"><span class="spinner" aria-hidden="true"></span> {t('upd.restarting')}</p>
    {:else if about.installing}
      <p class="small"><span class="spinner" aria-hidden="true"></span> {t('upd.installing', { version: about.latest?.version ?? '' })}</p>
      {#if about.progress}
        <p class="small muted">{t('upd.progress', { done: mb(about.progress[0]), total: mb(about.progress[1]) })}</p>
        <progress max={about.progress[1]} value={about.progress[0]}></progress>
      {/if}
    {:else if about.update_available && about.latest}
      <p class="small"><strong>{t('upd.available', { version: about.latest.version })}</strong></p>
      {#if about.latest.notes}
        <details>
          <summary class="small">{t('upd.whats_new')}</summary>
          <p class="small notes">{about.latest.notes}</p>
        </details>
      {/if}
      {#if about.cannot_install}
        <p class="small muted">{about.cannot_install}</p>
        <a class="small" href={about.latest.url} target="_blank" rel="noopener noreferrer">{t('upd.download')}</a>
      {:else}
        <button class="primary" onclick={install}>{t('upd.install')}</button>
      {/if}
    {:else if about.checked_at}
      <p class="small muted">
        {about.latest ? t('upd.up_to_date') : t('upd.no_releases')} {t('upd.checked', { when: ago(about.checked_at) })}
      </p>
    {:else}
      <p class="small muted">{t('upd.not_checked')}</p>
    {/if}
    {#if about.error}<p class="small error">{about.error}</p>{/if}
    {#if !about.installing && !restarting}
      <button class="small-button" disabled={checking} onclick={check}>
        {checking ? t('common.checking') : t('upd.check_now')}
      </button>
    {/if}

    <div class="options">
      <label class="channel">
        <span class="small">{t('upd.channel')}</span>
        <select
          value={about.nightly ? 'nightly' : 'stable'}
          onchange={(e) => setUpdates({ nightly: e.currentTarget.value === 'nightly' })}
        >
          <option value="stable">{t('upd.stable')}</option>
          <option value="nightly">{t('upd.nightly')}</option>
        </select>
      </label>
      <p class="small muted">
        {#if about.nightly}
          {t('upd.nightly_note')}
        {:else}
          {t('upd.stable_note')}
        {/if}
        {#if about.nightly_build && !about.nightly}
          {t('upd.stays')}
        {/if}
      </p>
      <label class="switch">
        <input
          type="checkbox"
          checked={about.check_updates}
          onchange={(e) => setUpdates({ check_updates: e.currentTarget.checked })}
        />
        {t('upd.daily')}
      </label>
      <label class="switch">
        <input
          type="checkbox"
          checked={about.auto_update}
          disabled={!about.check_updates}
          onchange={(e) => setUpdates({ auto_update: e.currentTarget.checked })}
        />
        {t('upd.auto')}
      </label>
      <p class="small muted">
        {t('upd.privacy')}
      </p>
    </div>
  </section>

{/if}

<style>
  progress {
    width: 100%;
    accent-color: var(--accent);
  }

  .error {
    color: var(--bad);
  }

  .notes {
    max-height: 240px;
    overflow-y: auto;
    white-space: pre-wrap;
  }

  .options {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding-top: 10px;
    border-top: 1px solid var(--line);
  }

  .small-button {
    align-self: flex-start;
  }

  .channel {
    display: flex;
    align-items: center;
    gap: 10px;
  }
</style>
