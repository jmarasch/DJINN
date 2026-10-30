<script lang="ts">
  // Admin > GO: optional converting to MP3, for devices that can't play a
  // format, saving phone data, and Subsonic apps asking for less.
  import { onMount } from 'svelte'
  import { api } from './api'
  import { toast } from './state.svelte'
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
      toast(e instanceof Error ? e.message : 'Could not load', true)
    }
  })

  async function save() {
    if (!form) return
    busy = true
    try {
      const s = await api.saveConvertSettings(form)
      used = s.cache_used_mb
      toast(s.enabled ? 'Converting is on' : 'Saved')
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not save', true)
    } finally {
      busy = false
    }
  }
</script>

{#if form}
  <section class="card">
    <h2>Converting</h2>
    <p class="small muted">
      Off unless you turn it on. Then DJINN makes MP3 copies for devices that can't play a song's format, for people who turn on
      "Save data" in GO, and for phone apps asking for a lower quality. Each song is converted once and kept, so it uses this computer's
      processor the first time only. Roles choose who can use it (under Users).
    </p>
    <label class="switch big">
      <input type="checkbox" bind:checked={form.enabled} onchange={save} disabled={busy} />
      <span>Converting is {form.enabled ? 'on' : 'off'}</span>
    </label>
    <div class="grid">
      <label>
        Quality
        <select bind:value={form.kbps} onchange={save}>
          <option value={96}>96 kbit/s (smallest)</option>
          <option value={128}>128 kbit/s</option>
          <option value={160}>160 kbit/s</option>
          <option value={192}>192 kbit/s (good)</option>
          <option value={256}>256 kbit/s</option>
          <option value={320}>320 kbit/s (best)</option>
        </select>
      </label>
      <label>
        At once
        <select bind:value={form.jobs} onchange={save}>
          {#each [1, 2, 3, 4] as n (n)}<option value={n}>{n}</option>{/each}
        </select>
      </label>
      <label>
        Keep up to (MB)
        <input type="number" min="100" step="100" bind:value={form.cache_mb} onchange={save} />
      </label>
    </div>
    <p class="small muted">
      Converted songs use {used} MB now; the oldest are removed past the limit. One conversion at a time keeps the party smooth on most
      computers.
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
