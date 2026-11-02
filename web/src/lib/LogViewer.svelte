<script lang="ts">
  // Admin > System: the latest log lines, to see what DJINN has been doing
  // or what went wrong.
  import { onMount } from 'svelte'
  import { api } from './api'
  import { toast } from './state.svelte'
  import type { LogSettings } from './types'

  const levels = [
    { value: 'error', label: 'Errors' },
    { value: 'warn', label: 'Warnings and errors' },
    { value: 'info', label: 'Everything' },
  ]
  let level = $state('warn')
  let search = $state('')
  let lines = $state<string[] | null>(null)
  let follow = $state(true)
  let box = $state<HTMLElement>()
  let settings = $state<LogSettings | null>(null)
  let days = $state<string[]>([])
  let saving = $state(false)

  onMount(() => {
    api.logSettings().then((value) => (settings = value)).catch((e) => toast(e.message, true))
    api.logFiles().then((value) => (days = value)).catch((e) => toast(e.message, true))
  })

  async function saveSettings() {
    if (!settings) return
    saving = true
    try {
      settings = await api.saveLogSettings(settings)
      toast('Log settings saved')
      await load()
      days = await api.logFiles()
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not save log settings', true)
    } finally {
      saving = false
    }
  }

  /** "2026-10-29T20:23:09.760235-05:00" shows as "20:23:09", the DJINN
   * computer's time. */
  const short = (line: string) =>
    line.replace(/^\d{4}-\d\d-\d\dT(\d\d:\d\d:\d\d)\.\d+(?:Z|[+-]\d\d:\d\d)/, '$1')

  async function load() {
    try {
      lines = (await api.systemLog(level, search.trim())).lines
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not read the log', true)
    }
  }

  // Reload when the filter changes (after typing stops), and every few
  // seconds while following.
  $effect(() => {
    void level
    void search
    const soon = setTimeout(load, 300)
    return () => clearTimeout(soon)
  })
  $effect(() => {
    if (!follow) return
    const timer = setInterval(load, 3000)
    return () => clearInterval(timer)
  })

  // Keep the newest line in view while following.
  $effect(() => {
    void lines
    if (follow && box) box.scrollTop = box.scrollHeight
  })
</script>

<section class="card">
  <h2>Log</h2>
  {#if settings}
    <div class="record-settings">
      <label>
        Record in the log
        <select bind:value={settings.level}>
          <option value="trace">Full tracing</option>
          <option value="warn">Warnings and errors</option>
          <option value="error">Errors only</option>
        </select>
      </label>
      <label>
        Days to keep
        <input type="number" min="1" max="365" step="1" bind:value={settings.keep_days} />
      </label>
      <button class="small-button" onclick={saveSettings} disabled={saving || !Number.isInteger(settings.keep_days) || settings.keep_days < 1 || settings.keep_days > 365}>Save log settings</button>
    </div>
    <p class="small muted">DJINN starts a dated log each day and removes logs older than the number of days you keep.</p>
  {/if}
  <div class="filters">
    <label class="display-level">Show lines
      <select bind:value={level}>
        {#each levels as l (l.value)}
          <option value={l.value}>{l.label}</option>
        {/each}
      </select>
    </label>
    <input type="search" bind:value={search} placeholder="Search the log" aria-label="Search the log" />
  </div>
  <pre bind:this={box} class="lines">{#if lines === null}Loading...{:else if lines.length}{lines.map(short).join(
        '\n',
      )}{:else}Nothing{search.trim() ? ' matches' : level === 'info' ? ' yet' : ' to worry about'}.{/if}</pre>
  <div class="actions">
    <label class="switch small">
      <input type="checkbox" bind:checked={follow} />
      Keep up with new lines
    </label>
    <a class="button small-button" href="/api/system/log/file" download>Download the log</a>
  </div>
  {#if days.length > 0}
    <details class="daily-files">
      <summary>Daily logs ({days.length})</summary>
      <div>{#each days as day (day)}<a href={`/api/system/log/files/${day}`} download>{day}</a>{/each}</div>
    </details>
  {/if}
  <p class="small muted">
    The latest lines since DJINN started, with times as the DJINN computer's clock shows them. Attach the
    download to a problem report; it can include song names and the addresses of devices on your network.
  </p>
</section>

<style>
  .record-settings {
    display: flex;
    flex-wrap: wrap;
    align-items: end;
    gap: 10px;
    margin-bottom: 10px;
  }

  .record-settings label {
    display: grid;
    gap: 4px;
  }

  .record-settings input {
    width: 100px;
  }

  .filters {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }

  .filters input {
    flex: 1;
    min-width: 160px;
  }

  .display-level { display: grid; gap: 4px; font-size: 0.875rem; }

  .lines {
    height: 320px;
    margin: 10px 0;
    padding: 10px;
    overflow: auto;
    border-radius: 8px;
    background: var(--bg);
    font-size: 0.75rem;
    line-height: 1.5;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }

  .daily-files { margin-top: 8px; font-size: 0.875rem; }
  .daily-files div { display: flex; flex-wrap: wrap; gap: 8px; margin-top: 6px; }

  p {
    margin: 8px 0 0;
  }
</style>
