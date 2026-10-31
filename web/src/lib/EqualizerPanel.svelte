<script lang="ts">
  import { onMount } from 'svelte'
  import { api } from './api'
  import { toast } from './state.svelte'
  import type { EqualizerSettings } from './types'

  const bands = [
    { name: 'Bass', frequency: '60 Hz' },
    { name: 'Low mids', frequency: '250 Hz' },
    { name: 'Mids', frequency: '1 kHz' },
    { name: 'Presence', frequency: '4 kHz' },
    { name: 'Treble', frequency: '12 kHz' },
  ]

  let settings = $state<EqualizerSettings>({ enabled: false, bands_db: [0, 0, 0, 0, 0] })
  let loaded = $state(false)
  let saving = $state(false)
  let timer: ReturnType<typeof setTimeout>
  let inFlight = Promise.resolve()
  let pending = 0

  onMount(() => {
    api.equalizer()
      .then((found) => {
        settings = found
        loaded = true
      })
      .catch((e) => toast(e instanceof Error ? e.message : 'Could not load equalizer', true))
    return () => clearTimeout(timer)
  })

  function save(delay = 0) {
    clearTimeout(timer)
    timer = setTimeout(() => {
      const next: EqualizerSettings = { enabled: settings.enabled, bands_db: [...settings.bands_db] }
      pending++
      saving = true
      inFlight = inFlight
        .then(() => api.saveEqualizer(next))
        .catch((e) => {
          toast(e instanceof Error ? e.message : 'Could not save equalizer', true)
        })
        .then(() => {
          pending--
          saving = pending > 0
        })
    }, delay)
  }

  function flat() {
    settings.bands_db = [0, 0, 0, 0, 0]
    save()
  }

  const formatted = (gain: number) => `${gain > 0 ? '+' : ''}${gain.toFixed(1)} dB`
</script>

<section class="card">
  <div class="heading">
    <h2>Equalizer</h2>
    {#if loaded}<button class="small-button" onclick={flat} disabled={settings.bands_db.every((gain) => gain === 0)}>Reset flat</button>{/if}
  </div>
  <p class="small muted">Shape the party mix on every output, including casting and Tune In. Boosts automatically leave headroom.</p>
  {#if loaded}
    <label class="switch big">
      <input type="checkbox" bind:checked={settings.enabled} onchange={() => save()} />
      Equalizer on
    </label>
    <div class="bands">
      {#each bands as band, i (band.frequency)}
        <label class="band">
          <span class="band-head">
            <span><strong>{band.name}</strong> <span class="small muted">{band.frequency}</span></span>
            <span class="gain">{formatted(settings.bands_db[i])}</span>
          </span>
          <input type="range" min="-12" max="12" step="0.5" bind:value={settings.bands_db[i]} oninput={() => save(250)} />
        </label>
      {/each}
    </div>
    <p class="small muted status" aria-live="polite">{saving ? 'Saving…' : settings.enabled ? 'Changes apply while music plays.' : 'Turn on to hear these settings.'}</p>
  {/if}
</section>

<style>
  .heading,
  .band-head {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 12px;
  }

  h2 { margin: 0; }

  .bands {
    display: grid;
    gap: 14px;
    margin-top: 18px;
  }

  .band {
    display: grid;
    gap: 4px;
  }

  .band-head { font-size: 0.875rem; }
  .gain { font-variant-numeric: tabular-nums; }
  .band input { width: 100%; accent-color: var(--accent); }
  .status { margin-bottom: 0; }
</style>
