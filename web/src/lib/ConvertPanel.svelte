<script lang="ts">
  // Admin > GO: optional converting to MP3, for devices that can't play a
  // format, saving phone data, and Subsonic apps asking for less.
  import { onMount } from 'svelte'
  import { api } from './api'
  import { toast } from './state.svelte'
  import { locale, t } from './i18n.svelte'
  import type { ConvertSettings } from './types'

  let form = $state<ConvertSettings | null>(null)
  let used = $state(0)
  let busy = $state(false)

  onMount(async () => {
    try {
      const s = await api.convertSettings()
      used = s.cache_used_mb
      form = { enabled: s.enabled, kbps: s.kbps, jobs: s.jobs, cache_mb: s.cache_mb }
    } catch (e) {
      toast(e instanceof Error ? e.message : t('ed.load_failed'), true)
    }
  })

  async function save() {
    if (!form) return
    busy = true
    try {
      const s = await api.saveConvertSettings(form)
      used = s.cache_used_mb
      toast(s.enabled ? t('cv.on') : t('common.saved'))
    } catch (e) {
      toast(e instanceof Error ? e.message : t('common.save_failed_short'), true)
    } finally {
      busy = false
    }
  }
</script>

{#if form}
  <section class="card">
    <h2>{t('cv.title')}</h2>
    <p class="small muted">
      {t('cv.intro')}
    </p>
    <label class="switch big">
      <input type="checkbox" bind:checked={form.enabled} onchange={save} disabled={busy} />
      <span>{form.enabled ? t('cv.is_on') : t('cv.is_off')}</span>
    </label>
    <div class="grid">
      <label>
        {t('cv.quality')}
        <select bind:value={form.kbps} onchange={save}>
          <option value={96}>{t('cv.smallest', { n: 96 })}</option>
          <option value={128}>128 kbit/s</option>
          <option value={160}>160 kbit/s</option>
          <option value={192}>{t('cv.good', { n: 192 })}</option>
          <option value={256}>256 kbit/s</option>
          <option value={320}>{t('cv.best', { n: 320 })}</option>
        </select>
      </label>
      <label>
        {t('cv.at_once')}
        <select bind:value={form.jobs} onchange={save}>
          {#each [1, 2, 3, 4] as n (n)}<option value={n}>{n}</option>{/each}
        </select>
      </label>
      <label>
        {t('cv.keep')}
        <input type="number" min="100" step="100" bind:value={form.cache_mb} onchange={save} />
      </label>
    </div>
    <p class="small muted">
      {t('cv.used', { n: used.toLocaleString(locale.code) })}
    </p>
  </section>
{/if}

<style>
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(140px, 1fr));
    gap: 10px;
  }

  .switch.big {
    align-items: flex-start;
  }

  p {
    margin: 0;
  }
</style>
