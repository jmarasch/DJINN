<script lang="ts">
  // Admin > Visualizers: what node screens draw, for the whole house. Kept
  // on this server and sent to each node; a node can have its own instead
  // (Admin > Nodes > Screen, or the node's own page).
  import { t } from './i18n.svelte'
  import { onMount } from 'svelte'
  import { api } from './api'
  import { toast } from './state.svelte'
  import type { Visuals } from './visuals'
  import VisualsEditor from './VisualsEditor.svelte'

  let visuals = $state<Visuals | null>(null)
  let failed = $state(false)

  onMount(async () => {
    try {
      visuals = await api.visuals()
    } catch {
      failed = true
    }
  })

  async function save(next: Visuals) {
    const before = visuals
    visuals = next
    try {
      visuals = await api.setVisuals(next)
    } catch (e) {
      visuals = before
      toast(e instanceof Error ? e.message : t('common.save_failed_short'), true)
    }
  }
</script>

<section class="card">
  <h2>{t('vp.title')}</h2>
  <p class="small muted">
    {t('vp.intro')}
  </p>
  {#if failed}
    <p class="error">{t('vp.failed')}</p>
  {:else if visuals}
    <VisualsEditor value={visuals} onchange={save} />
  {/if}
</section>
