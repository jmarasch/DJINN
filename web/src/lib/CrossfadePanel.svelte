<script lang="ts">
  import { t } from './i18n.svelte'
  import { api } from './api'
  import { app, toast } from './state.svelte'

  let crossfade = $state(app.player.crossfade_secs)
  // Follow changes made elsewhere.
  $effect(() => {
    crossfade = app.player.crossfade_secs
  })

  let timer: ReturnType<typeof setTimeout>
  function save() {
    clearTimeout(timer)
    timer = setTimeout(async () => {
      try {
        await api.setCrossfade(crossfade)
      } catch (e) {
        toast(e instanceof Error ? e.message : t('common.save_failed_short'), true)
      }
    }, 200)
  }
</script>

<section class="card">
  <h2>{t('xf.title')}</h2>
  <label class="slider">
    <span class="row-label">
      {t('xf.blend')} <span class="muted">{crossfade ? t('xf.secs', { n: crossfade }) : t('xf.off')}</span>
    </span>
    <input type="range" min="0" max="12" step="1" bind:value={crossfade} oninput={save} />
  </label>
  <p class="small muted">
    {t('xf.note')}
  </p>
</section>

<style>
  .slider {
    gap: 2px;
  }

  .row-label {
    display: flex;
    flex-direction: row;
    justify-content: space-between;
    font-size: 0.875rem;
  }
</style>
