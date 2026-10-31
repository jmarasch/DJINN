<script lang="ts">
  import { onMount } from 'svelte'
  import { api } from './api'
  import { toast } from './state.svelte'
  import type { EqualizerSettings } from './types'

  const bands = ['32', '64', '125', '250', '500', '1k', '2k', '4k', '6k', '8k', '12k', '16k']
  const presets = [
    { id: 'flat', label: 'Flat', gains: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0] },
    { id: 'bass', label: 'Bass Boost', gains: [4, 5, 5, 3, 1, 0, 0, 0, 0, 0, 0, 0] },
    { id: 'vocal', label: 'Vocals', gains: [0, -1, -1, 0, 1, 2, 3, 2, 1, 0, 0, 0] },
    { id: 'rock', label: 'Rock', gains: [3, 4, 3, 1, -1, 0, 2, 3, 2, 1, 1, 0] },
    { id: 'electronic', label: 'Electronic', gains: [4, 5, 3, 1, 0, -1, 0, 2, 3, 2, 1, 0] },
    { id: 'bright', label: 'Bright', gains: [0, 0, 0, 0, 0, 0, 1, 2, 2, 3, 3, 2] },
  ]

  let settings = $state<EqualizerSettings>({ enabled: false, bands_db: Array(12).fill(0), output_gain_db: 0 })
  let loaded = $state(false)
  let saving = $state(false)
  let timer: ReturnType<typeof setTimeout>
  let inFlight = Promise.resolve()
  let pending = 0
  const selectedPreset = $derived(
    presets.find((preset) => preset.gains.every((gain, i) => gain === settings.bands_db[i]))?.id ?? 'custom',
  )

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
      const next: EqualizerSettings = {
        enabled: settings.enabled,
        bands_db: [...settings.bands_db],
        output_gain_db: settings.output_gain_db,
      }
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
    settings.bands_db = [...presets[0].gains]
    settings.output_gain_db = 0
    save()
  }

  function usePreset(preset: (typeof presets)[number]) {
    settings.bands_db = [...preset.gains]
    settings.enabled = true
    save()
  }

  const formatted = (gain: number) => `${gain > 0 ? '+' : ''}${gain.toFixed(1)} dB`
</script>

<section class="card">
  <div class="heading">
    <h2>Equalizer</h2>
    {#if loaded}<button class="small-button" onclick={flat} disabled={selectedPreset === 'flat' && settings.output_gain_db === 0}>Reset flat</button>{/if}
  </div>
  <p class="small muted">Shape the party mix on every output, including casting and Tune In. Normal levels stay intact; peaks near clipping are softened.</p>
  {#if loaded}
    <label class="switch big">
      <input type="checkbox" bind:checked={settings.enabled} onchange={() => save()} />
      Equalizer on
    </label>
    <div class="presets" role="group" aria-label="Equalizer presets">
      {#each presets as preset (preset.id)}
        <button class:chosen={selectedPreset === preset.id} aria-pressed={selectedPreset === preset.id} onclick={() => usePreset(preset)}>{preset.label}</button>
      {/each}
      {#if selectedPreset === 'custom'}<span class="small muted">Custom</span>{/if}
    </div>
    <div class="bars" role="group" aria-label="Equalizer bands">
      <div class="zero-line" aria-hidden="true"></div>
      {#each bands as band, i (band)}
        <label class="band">
          <span class="gain">{formatted(settings.bands_db[i])}</span>
          <input type="range" min="-12" max="12" step="0.5" bind:value={settings.bands_db[i]} oninput={() => save(250)} aria-label="{band} Hz" />
          <span class="frequency">{band}</span>
        </label>
      {/each}
    </div>
    <label class="output-level">
      <span>Output level <span class="gain">{formatted(settings.output_gain_db)}</span></span>
      <input type="range" min="-12" max="6" step="0.5" bind:value={settings.output_gain_db} oninput={() => save(250)} />
    </label>
    <p class="small muted status" aria-live="polite">{saving ? 'Saving…' : settings.enabled ? 'Changes apply while music plays.' : 'Turn on to hear these settings.'}</p>
  {/if}
</section>

<style>
  .heading,
  .output-level > span {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 12px;
  }

  h2 { margin: 0; }

  .presets {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    margin: 18px 0;
  }

  .presets button {
    min-height: 36px;
    padding: 6px 12px;
    border: 1px solid var(--line);
    border-radius: 999px;
    background: var(--surface-2);
  }

  .presets button.chosen {
    border-color: var(--accent);
    color: var(--accent);
  }

  .bars {
    position: relative;
    display: grid;
    grid-template-columns: repeat(12, minmax(42px, 1fr));
    gap: 6px;
    overflow-x: auto;
    padding: 8px 0 14px;
  }

  .zero-line {
    position: absolute;
    top: 106px;
    right: 0;
    left: 0;
    border-top: 1px dashed var(--line);
    pointer-events: none;
  }

  .band {
    display: grid;
    justify-items: center;
    gap: 6px;
    min-width: 42px;
    font-size: 0.75rem;
  }

  .gain {
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  .band input {
    width: 24px;
    height: 180px;
    writing-mode: vertical-lr;
    direction: rtl;
    accent-color: var(--accent);
    cursor: pointer;
  }

  .frequency {
    color: var(--muted);
    white-space: nowrap;
  }

  .output-level {
    display: grid;
    gap: 6px;
    margin-top: 10px;
    font-size: 0.875rem;
  }

  .output-level input { width: 100%; accent-color: var(--accent); }
  .status { margin-bottom: 0; }
</style>
