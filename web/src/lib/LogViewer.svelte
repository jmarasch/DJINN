<script lang="ts">
  // Admin > System: the latest log lines, to see what DJINN has been doing
  // or what went wrong.
  import { api } from './api'
  import { toast } from './state.svelte'

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
  <div class="filters">
    <select bind:value={level} aria-label="Show">
      {#each levels as l (l.value)}
        <option value={l.value}>{l.label}</option>
      {/each}
    </select>
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
  <p class="small muted">
    The latest lines since DJINN started, with times as the DJINN computer's clock shows them. Attach the
    download to a problem report; it can include song names and the addresses of devices on your network.
  </p>
</section>

<style>
  .filters {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }

  .filters input {
    flex: 1;
    min-width: 160px;
  }

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

  p {
    margin: 8px 0 0;
  }
</style>
