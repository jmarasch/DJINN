<script lang="ts">
  import { api } from './api'
  import { app, toast } from './state.svelte'
  import { locale, t } from './i18n.svelte'
  import type { ReplayGainMode } from './types'

  let mode = $state<ReplayGainMode>('off')
  let preamp = $state(0)
  let maxPreamp = $state(12)
  let loaded = $state(false)

  $effect(() => {
    api
      .replayGain()
      .then((r) => {
        mode = r.mode
        preamp = r.preamp_db
        maxPreamp = r.max_preamp_db
        app.loudness = r.loudness
        loaded = true
      })
      .catch((e) => toast(e.message, true))
  })

  const modes: { id: ReplayGainMode; label: string; detail: string }[] = $derived([
    { id: 'off', label: t('rg.off'), detail: t('rg.off_note') },
    { id: 'track', label: t('rg.track'), detail: t('rg.track_note') },
    { id: 'album', label: t('rg.album'), detail: t('rg.album_note') },
  ])

  let timer: ReturnType<typeof setTimeout>
  function save(wait = 0) {
    clearTimeout(timer)
    timer = setTimeout(async () => {
      try {
        const r = await api.saveReplayGain(mode, preamp)
        app.loudness = r.loudness
      } catch (e) {
        toast(e instanceof Error ? e.message : t('common.save_failed_short'), true)
      }
    }, wait)
  }

  const progress = $derived(app.loudness)
  const total = $derived(progress ? progress.analyzed + progress.waiting : 0)
</script>

<section class="card">
  <h2>{t('rg.title')}</h2>
  <p class="small muted">
    {t('rg.intro')}
  </p>
  {#if loaded}
    <div class="modes" role="radiogroup" aria-label={t('rg.group')}>
      {#each modes as m (m.id)}
        <label class="mode" class:active={mode === m.id}>
          <input type="radio" name="replaygain" value={m.id} bind:group={mode} onchange={() => save()} />
          <span>
            <span class="label">{m.label}</span>
            <span class="small muted">{m.detail}</span>
          </span>
        </label>
      {/each}
    </div>

    {#if mode !== 'off'}
      <label class="slider">
        <span class="row-label">
          {t('rg.boost')} <span class="muted">{preamp > 0 ? '+' : ''}{preamp.toLocaleString(locale.code, { minimumFractionDigits: 1, maximumFractionDigits: 1 })} dB</span>
        </span>
        <input type="range" min={-maxPreamp} max={maxPreamp} step="0.5" bind:value={preamp} oninput={() => save(400)} />
        <span class="small muted">
          {t('rg.boost_note')}
        </span>
      </label>

      {#if progress && !progress.on}
        <p class="small muted">
          {t('rg.analysis_off')} <a href="#/admin/library">{t('admin.menu.library')}</a>.
        </p>
      {:else if progress && total > 0}
        {#if progress.waiting > 0}
          <p class="small">
            {#if progress.running}<span class="spinner" aria-hidden="true"></span>{/if}
            {t('rg.measured', { done: progress.analyzed, total })}{progress.running ? '' : t('rg.continues')}.
            {t('rg.not_yet')}
          </p>
          <progress max={total} value={progress.analyzed}></progress>
        {:else}
          <p class="small muted">{t('rg.all', { n: total })}</p>
        {/if}
      {/if}
    {/if}
  {/if}
</section>

<style>
  .modes {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .mode {
    flex-direction: row;
    align-items: flex-start;
    gap: 10px;
    padding: 10px 12px;
    border: 1px solid var(--line);
    border-radius: 10px;
    cursor: pointer;
  }

  .mode.active {
    border-color: var(--accent);
  }

  .mode input {
    flex: none;
    width: 20px;
    min-height: 20px;
    margin: 1px 0 0;
    accent-color: var(--accent);
  }

  .mode > span {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .label {
    font-weight: 600;
  }

  .slider {
    gap: 4px;
  }

  .row-label {
    display: flex;
    flex-direction: row;
    justify-content: space-between;
    font-size: 0.875rem;
  }

  progress {
    width: 100%;
    accent-color: var(--accent);
  }
</style>
