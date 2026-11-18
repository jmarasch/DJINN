<script lang="ts">
  // A node's page, while managing it: what the node says about itself, its
  // log, a restart, and restarting the whole computer. Talks to the node's own DJINN, not Main.
  import { onMount } from 'svelte'
  import { locale, t } from './i18n.svelte'

  interface Diagnostics {
    version: string
    build: string
    label: string
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
    can_reboot: boolean
  }

  let facts = $state<Diagnostics | null>(null)
  let lines = $state<string[]>([])
  let level = $state('info')
  let search = $state('')
  let follow = $state(true)
  let error = $state('')
  let restarting = $state(false)
  let confirmRestart = $state(false)
  let rebooting = $state(false)
  let confirmReboot = $state(false)
  let logBox = $state<HTMLElement>()

  const gb = (bytes: number) =>
    `${(bytes / 1024 ** 3).toLocaleString(locale.code, { minimumFractionDigits: 1, maximumFractionDigits: 1 })} GB`
  const mb = (bytes: number) => `${Math.round(bytes / 1024 ** 2).toLocaleString(locale.code)} MB`

  function uptime(secs: number) {
    const d = Math.floor(secs / 86400)
    const h = Math.floor((secs % 86400) / 3600)
    const m = Math.floor((secs % 3600) / 60)
    return d ? t('nd.d_h', { d, h }) : h ? t('nd.h_m', { h, m }) : t('nd.m', { m })
  }

  async function loadFacts() {
    try {
      const res = await fetch('/api/node/diagnostics')
      if (!res.ok) throw new Error((await res.json().catch(() => null))?.error ?? t('nd.load_failed'))
      facts = await res.json()
      error = ''
    } catch (e) {
      error = e instanceof Error ? e.message : t('nd.load_failed')
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

  async function reboot() {
    confirmReboot = false
    const res = await fetch('/api/node/reboot', { method: 'POST' }).catch(() => null)
    if (!res?.ok) {
      error = (await res?.json().catch(() => null))?.error ?? t('nd.reboot_failed')
      return
    }
    error = ''
    rebooting = true
    // Off for a minute or two; the page's status check shows it coming back.
    setTimeout(() => {
      rebooting = false
      void loadFacts()
    }, 120000)
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
  <h2>{t('nd.title')}</h2>
  {#if error}<p class="error">{error}</p>{/if}
  {#if facts}
    <dl>
      <dt>{t('nd.version')}</dt>
      <dd>{facts.version} <span class="muted small">{t('nd.build', { build: facts.build })}</span>{#if facts.label}
          <b>{facts.label}</b>{/if}</dd>
      <dt>{t('nd.system')}</dt>
      <dd>{t('nd.threads', { platform: facts.platform, n: facts.threads })}</dd>
      <dt>{t('nd.uptime')}</dt>
      <dd>{uptime(facts.uptime_secs)}</dd>
      {#if facts.addresses.length}
        <dt>{t('nd.addresses')}</dt>
        <dd>{facts.addresses.join(', ')}</dd>
      {/if}
      {#if facts.cpu_busy != null}
        <dt>{t('admin.home.processor')}</dt>
        <dd>{t('admin.home.busy', { pct: Math.round(facts.cpu_busy * 100) })}</dd>
      {/if}
      {#if facts.memory_total != null && facts.memory_available != null}
        <dt>{t('admin.home.memory')}</dt>
        <dd>{t('admin.home.free_of', { free: gb(facts.memory_available), total: gb(facts.memory_total) })}</dd>
      {/if}
      {#if facts.disk_total != null && facts.disk_available != null}
        <dt>{t('admin.home.disk')}</dt>
        <dd>{t('admin.home.free_of', { free: gb(facts.disk_available), total: gb(facts.disk_total) })}</dd>
      {/if}
      <dt>{t('nd.song_cache')}</dt>
      <dd>{t('nd.cache_files', { n: facts.cached_songs.toLocaleString(locale.code), size: mb(facts.cached_bytes) })}</dd>
    </dl>
  {/if}
  <div class="buttons">
    {#if confirmRestart}
      <span class="small">{t('nd.restart_q')}</span>
      <button class="danger" onclick={restart}>{t('nd.restart')}</button>
      <button onclick={() => (confirmRestart = false)}>{t('common.cancel')}</button>
    {:else}
      <button disabled={restarting} onclick={() => (confirmRestart = true)}>
        {restarting ? t('nd.restarting') : t('nd.restart_node')}
      </button>
    {/if}
  </div>
  {#if facts?.can_reboot}
    <div class="buttons">
      {#if confirmReboot}
        <span class="small">
          {t('nd.reboot_q')}
        </span>
        <button class="danger" onclick={reboot}>{t('nd.reboot')}</button>
        <button onclick={() => (confirmReboot = false)}>{t('common.cancel')}</button>
      {:else}
        <button disabled={rebooting} onclick={() => (confirmReboot = true)}>
          {rebooting ? t('nd.rebooting') : t('nd.reboot')}
        </button>
      {/if}
    </div>
  {:else if facts}
    <p class="small muted">
      {t('nd.no_reboot')}
    </p>
  {/if}
</section>

<section class="card">
  <h2>{t('nd.log')}</h2>
  <div class="filters">
    <label>
      <span class="small muted">{t('nd.show')}</span>
      <select bind:value={level}>
        <option value="trace">{t('nd.trace')}</option>
        <option value="debug">{t('nd.debug')}</option>
        <option value="info">{t('nd.info')}</option>
        <option value="warn">{t('nd.warn')}</option>
        <option value="error">{t('nd.error')}</option>
      </select>
    </label>
    <input type="search" bind:value={search} placeholder={t('nd.search')} aria-label={t('nd.search')} />
    <label class="follow"><input type="checkbox" bind:checked={follow} /> {t('nd.follow')}</label>
    <a class="button" href="/api/node/log/file" download>{t('nd.download')}</a>
  </div>
  {#if facts && (facts.log_level === 'warn' || facts.log_level === 'error')}
    <p class="small muted">{facts.log_level === 'warn' ? t('nd.only_warn') : t('nd.only_error')}</p>
  {/if}
  <pre class="log" bind:this={logBox}>{lines.length ? lines.join('\n') : t('nd.nothing')}</pre>
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
