<script lang="ts">
  // Admin > Visualizers: what node screens draw, for the whole house. Kept
  // on this server and sent to each node; a node can have its own instead
  // (Admin > Nodes > Screen, or the node's own page).
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
      toast(e instanceof Error ? e.message : 'Could not save', true)
    }
  }
</script>

<section class="card">
  <h2>Visualizers</h2>
  <p class="small muted">
    What a node's screen draws while it shows the visualizer (Admin &gt; Nodes &gt; Screen). Each node measures its own
    speaker and draws it in step. These are for every node, unless a node has its own.
  </p>
  {#if failed}
    <p class="error">Could not load the visualizers.</p>
  {:else if visuals}
    <VisualsEditor value={visuals} onchange={save} />
  {/if}
</section>
