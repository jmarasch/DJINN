<script lang="ts">
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
        toast(e instanceof Error ? e.message : 'Could not save', true)
      }
    }, 200)
  }
</script>

<section class="card">
  <h2>Crossfade</h2>
  <label class="slider">
    <span class="row-label">
      Blend songs into each other <span class="muted">{crossfade ? `${crossfade} s` : 'Off (gapless)'}</span>
    </span>
    <input type="range" min="0" max="12" step="1" bind:value={crossfade} oninput={save} />
  </label>
  <p class="small muted">
    Songs from the same album always join without a gap, so live albums and DJ mixes flow as recorded.
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
