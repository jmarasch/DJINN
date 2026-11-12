<script lang="ts">
  // A node's page, while managing it: what the node says about itself, its
  // log, and a restart. Talks to the node's own DJINN, not Main.
  import { onMount } from 'svelte'

  interface Diagnostics {
    version: string
    build: string
    platform: string
    uptime_secs: number
    addresses: string[]
    threads: number
    cpu_busy: number | null
    memory_total: number | null
    memory_available: number | null
    disk_total: number | null
    disk_available: number | null
    cached_songs: number
    cached_bytes: number
    log_level: string
  }

  let facts = $state<Diagnostics | null>(null)
  let lines = $state<string[]>([])
  let level = $state('info')
  let search = $state('')
  let follow = $state(true)
  let error = $state('')
  let restarting = $state(false)
  let confirmRestart = $state(false)
  let logBox = $state<HTMLElement>()

  const gb = (bytes: number) => `${(bytes / 1024 ** 3).toFixed(1)} GB`
  const mb = (bytes: number) => `${Math.round(bytes / 1024 ** 2).toLocaleString()} MB`

  function uptime(secs: number) {
    const d = Math.floor(secs / 86400)
    const h = Math.floor((secs % 86400) / 3600)
    const m = Math.floor((secs % 3600) / 60)
    return d ? `${d} d ${h} h` : h ? `${h} h ${m} min` : `${m} min`
  }

  async function loadFacts() {
    try {
      const res = await fetch('/api/node/diagnostics')
      if (!res.ok) throw new Error((await res.json().catch(() => null))?.error ?? 'Could not load')
      facts = await res.json()
      error = ''
    } catch (e) {
      error = e instanceof Error ? e.message : 'Could not load'
    }
  }

  async function loadLog() {
    const query = new URLSearchParams({ level })
    if (search.trim()) query.set('search', search.trim())
    try {
      const res = await fetch(`/api/node/log?${query}`)
      if (!res.ok) return
      const atBottom = !logBox || logBox.scrollHeight - logBox.scrollTop - logBox.clientHeight < 40
      lines = await res.json()
      if (atBottom) requestAnimationFrame(() => logBox?.scrollTo({ top: logBox.scrollHeight }))
    } catch {
      // Tried again on the next tick.
    }
  }

  async function restart() {
    restarting = true
    confirmRestart = false
    await fetch('/api/node/restart', { method: 'POST' }).catch(() => null)
    // The page's status check shows it coming back.
    setTimeout(() => {
      restarting = false
      void loadFacts()
    }, 8000)
  }

  onMount(() => {
    void loadFacts()
    void loadLog()
    const timer = setInterval(() => {
      if (follow) void loadLog()
    }, 3000)
    const factsTimer = setInterval(loadFacts, 15000)
    return () => {
      clearInterval(timer)
      clearInterval(factsTimer)
    }
  })

  $effect(() => {
    void level
    void search
    void loadLog()
  })
</script>

<section class="card">
  <h2>Diagnostics</h2>
  {#if error}<p class="error">{error}</p>{/if}
  {#if facts}
    <dl>
      <dt>Version</dt>
      <dd>{facts.version} <span class="muted small">build {facts.build}</span></dd>
      <dt>System</dt>
      <dd>{facts.platform}, {facts.threads} threads</dd>
      <dt>Running for</dt>
      <dd>{uptime(facts.uptime_secs)}</dd>
      {#if facts.addresses.length}
        <dt>Addresses</dt>
        <dd>{facts.addresses.join(', ')}</dd>
      {/if}
      {#if facts.cpu_busy != null}
        <dt>Processor</dt>
        <dd>{Math.round(facts.cpu_busy * 100)}% busy</dd>
      {/if}
      {#if facts.memory_total != null && facts.memory_available != null}
        <dt>Memory</dt>
        <dd>{gb(facts.memory_available)} free of {gb(facts.memory_total)}</dd>
      {/if}
      {#if facts.disk_total != null && facts.disk_available != null}
        <dt>Disk</dt>
        <dd>{gb(facts.disk_available)} free of {gb(facts.disk_total)}</dd>
      {/if}
      <dt>Song cache</dt>
      <dd>{facts.cached_songs.toLocaleString()} files, {mb(facts.cached_bytes)}</dd>
    </dl>
  {/if}
  <div class="buttons">
    {#if confirmRestart}
      <span class="small">Restart this node? Its music stops for a few seconds.</span>
      <button class="danger" onclick={restart}>Restart</button>
      <button onclick={() => (confirmRestart = false)}>Cancel</button>
    {:else}
      <button disabled={restarting} onclick={() => (confirmRestart = true)}>
        {restarting ? 'Restarting...' : 'Restart node'}
      </button>
    {/if}
  </div>
</section>

<section class="card">
  <h2>Log</h2>
  <div class="filters">
    <label>
      <span class="small muted">Show</span>
      <select bind:value={level}>
        <option value="trace">Everything</option>
        <option value="debug">Debug and up</option>
        <option value="info">Info and up</option>
        <option value="warn">Warnings and errors</option>
        <option value="error">Errors only</option>
      </select>
    </label>
    <input type="search" bind:value={search} placeholder="Search the log" aria-label="Search the log" />
    <label class="follow"><input type="checkbox" bind:checked={follow} /> Keep updating</label>
    <a class="button" href="/api/node/log/file" download>Download log</a>
  </div>
  {#if facts && (facts.log_level === 'warn' || facts.log_level === 'error')}
    <p class="small muted">This node only records {facts.log_level === 'warn' ? 'warnings and errors' : 'errors'}.</p>
  {/if}
  <pre class="log" bind:this={logBox}>{lines.length ? lines.join('\n') : 'Nothing logged yet.'}</pre>
</section>

<style>
  section {
    margin-top: 12px;
  }

  h2 {
    margin: 0 0 8px;
    font-size: 1.125rem;
  }

  dl {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 4px 12px;
    margin: 0 0 12px;
  }

  dt {
    color: var(--muted);
  }

  dd {
    margin: 0;
    overflow-wrap: anywhere;
  }

  .buttons,
  .filters {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
  }

  .filters {
    margin-bottom: 8px;
  }

  .filters label {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }

  .filters input[type='search'] {
    flex: 1;
    min-width: 140px;
  }

  .log {
    max-height: 60vh;
    margin: 0;
    padding: 10px;
    overflow: auto;
    border-radius: 8px;
    background: var(--surface-2);
    font-size: 0.75rem;
    line-height: 1.4;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
</style>
