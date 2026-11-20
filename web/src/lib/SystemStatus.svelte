<script lang="ts">
  // Admin > System: how DJINN is doing, and restarting or quitting it.
  import { onMount } from 'svelte'
  import { api } from './api'
  import { reloadAfterRestart } from './restart'
  import { toast } from './state.svelte'
  import { locale, t } from './i18n.svelte'
  import type { ServerPorts, SystemStatus } from './types'

  let status = $state<SystemStatus | null>(null)
  /** Restarting, or quit: DJINN is going away. */
  let leaving = $state<'restart' | 'quit' | null>(null)
  let ports = $state<ServerPorts | null>(null)
  let portForm = $state({ party_port: 8066, go_port: 8067 })
  let savingPorts = $state(false)
  let restartTarget = $state<string | undefined>(undefined)

  onMount(() => {
    api.systemPorts().then((value) => {
      ports = value
      portForm = { party_port: value.pending_party_port, go_port: value.pending_go_port }
    }).catch((e) => toast(e.message, true))
  })

  async function load() {
    try {
      status = await api.systemStatus()
    } catch (e) {
      if (!leaving) toast(e instanceof Error ? e.message : t('admin.home.load_failed'), true)
    }
  }

  $effect(() => {
    void load()
    const timer = setInterval(() => !leaving && void load(), 10_000)
    return () => clearInterval(timer)
  })

  $effect(() => {
    if (leaving === 'restart') return reloadAfterRestart(restartTarget)
  })

  async function savePorts() {
    savingPorts = true
    try {
      ports = await api.saveSystemPorts(portForm)
      portForm = { party_port: ports.pending_party_port, go_port: ports.pending_go_port }
      toast(t('ss.ports_saved'))
    } catch (e) {
      toast(e instanceof Error ? e.message : t('ss.ports_failed'), true)
    } finally {
      savingPorts = false
    }
  }

  async function restart() {
    if (!confirm(t('ss.restart_q'))) return
    try {
      restartTarget = undefined
      if (ports && ports.pending_party_port !== ports.party_port && Number(location.port) === ports.party_port) {
        const target = new URL(location.href)
        target.port = String(ports.pending_party_port)
        restartTarget = target.href
      }
      await api.restart()
      leaving = 'restart'
    } catch (e) {
      toast(e instanceof Error ? e.message : t('ss.restart_failed'), true)
    }
  }

  async function quit() {
    if (!confirm(t('ss.quit_q')))
      return
    try {
      await api.quit()
      leaving = 'quit'
    } catch (e) {
      toast(e instanceof Error ? e.message : t('ss.quit_failed'), true)
    }
  }

  const size = (bytes: number) =>
    bytes >= 1e12
      ? `${num(bytes / 1e12, 2)} TB`
      : bytes >= 1e9
        ? `${num(bytes / 1e9, 1)} GB`
        : `${num(bytes / 1e6, bytes >= 1e7 ? 0 : 1)} MB`
  /** A number with this many decimals, in the language's own way. */
  const num = (x: number, digits: number) =>
    x.toLocaleString(locale.code, { minimumFractionDigits: digits, maximumFractionDigits: digits })

  function uptime(since: number): string {
    const minutes = Math.floor((Date.now() - since) / 60_000)
    if (minutes < 1) return t('ss.under_minute')
    if (minutes < 60) return t('ss.min', { n: minutes })
    const hours = Math.floor(minutes / 60)
    if (hours < 48) return t('ss.h_min', { h: hours, m: minutes % 60 })
    return t('ss.days', { n: Math.floor(hours / 24) })
  }

  const stateLabel = $derived({
    idle: t('admin.home.nothing_playing'),
    playing: t('admin.home.playing'),
    paused: t('admin.home.paused'),
  })
</script>

<section class="card">
  <h2>{t('ss.title')}</h2>
  {#if leaving === 'restart'}
    <p class="small"><span class="spinner" aria-hidden="true"></span> {t('ss.restarting')}</p>
  {:else if leaving === 'quit'}
    <p class="small">{t('ss.quit')}</p>
  {:else if status}
    <dl class="facts small">
      <dt>DJINN</dt>
      <dd>
        {status.version}{#if status.started_ms}{t('ss.running_for', { time: uptime(status.started_ms) })}{/if}
      </dd>

      <dt>{t('ss.phones_open')}</dt>
      <dd>
        {#each status.addresses as address (address)}
          <div><a href={address} target="_blank" rel="noopener noreferrer">{address}</a></div>
        {:else}
          <span class="muted">{t('ss.unknown')}</span>
        {/each}
        {#if status.public_network}
          <div class="bad">{t('admin.home.public_network')}.</div>
        {/if}
      </dd>

      <dt>{t('admin.library')}</dt>
      <dd>
        {t('ss.library_line', {
          songs: status.songs.toLocaleString(locale.code),
          artists: status.artists.toLocaleString(locale.code),
          albums: status.albums.toLocaleString(locale.code),
          size: size(status.library_bytes),
          folders: t('admin.home.n_folders', { n: status.library_folders }),
        })}{#if status.scanning}{t('ss.checking')}{/if}
      </dd>

      <dt>{t('admin.home.music')}</dt>
      <dd>{stateLabel[status.state]}</dd>

      <dt>{t('ss.speakers')}</dt>
      <dd>
        {#each status.outputs.outputs.filter((o) => o.state !== 'disabled') as output (output.id)}
          <div>
            {output.device ?? t('ss.waiting_device')}{#if output.state === 'waiting'}
              <span class="muted">{t('ss.waiting')}</span>{/if}
            {#if output.error}<span class="bad"> {output.error}</span>{/if}
          </div>
        {:else}
          <span class="muted">{t('ss.none_on')}</span>
        {/each}
        {#if status.outputs.error}<div class="bad">{status.outputs.error}</div>{/if}
      </dd>

      <dt>{t('ss.casting_to')}</dt>
      <dd>
        {#each status.casts as cast (cast.name)}
          <div>
            {cast.name}{#if cast.error}<span class="bad"> {cast.error}</span>{:else if !cast.playing}<span class="muted">
                {t('ss.connecting')}</span
              >{/if}
          </div>
        {:else}
          <span class="muted">{t('ss.nothing')}</span>
        {/each}
      </dd>

      <dt>AI</dt>
      <dd>
        {t('ss.ai_line', {
          listening: status.ai_listening ? t('ss.loaded') : t('common.off'),
          reader: status.ai_reader ? t('ss.loaded') : t('common.off'),
        })}
      </dd>

      <dt>{t('ss.storage')}</dt>
      <dd>
        {t('ss.storage_line', { db: size(status.database_bytes), art: size(status.art_bytes), models: size(status.models_bytes) })}
        {#if status.data_dir}<div class="muted path">{status.data_dir}</div>{/if}
      </dd>

      {#if status.log_file}
        <dt>{t('ss.log_file')}</dt>
        <dd class="muted path">{status.log_file}</dd>
      {/if}
    </dl>

    {#if ports}
      <div class="port-settings">
        <h3>{t('ss.ports')}</h3>
        <div class="port-fields">
          <label>{t('ss.party')}
            <input type="number" min="1" max="65535" step="1" bind:value={portForm.party_port} />
          </label>
          <label>DJINN GO
            <input type="number" min="1" max="65535" step="1" bind:value={portForm.go_port} />
          </label>
          <button onclick={savePorts} disabled={savingPorts || !Number.isInteger(portForm.party_port) || !Number.isInteger(portForm.go_port) || portForm.party_port < 1 || portForm.go_port < 1 || portForm.party_port > 65535 || portForm.go_port > 65535 || portForm.party_port === portForm.go_port}>{t('ss.save_ports')}</button>
        </div>
        <p class="small muted">{t('ss.ports_note', { party: ports.party_port, go: ports.go_port })}</p>
      </div>
    {/if}

    <div class="actions">
      <button onclick={restart}>{t('ss.restart')}</button>
      <button class="danger" onclick={quit}>{t('ss.quit_button')}</button>
    </div>
    <p class="small muted">
      {t('ss.restart_note')}
    </p>
  {:else}
    <p class="small muted">{t('common.loading')}</p>
  {/if}
</section>

<style>
  .facts {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 8px 16px;
    margin: 0 0 12px;
  }

  dt {
    color: var(--muted);
  }

  dd {
    margin: 0;
    min-width: 0;
    overflow-wrap: anywhere;
  }

  .path {
    font-size: 0.8125rem;
  }

  .bad {
    color: var(--bad);
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }

  .port-settings { margin: 16px 0; }
  .port-settings h3 { margin: 0 0 8px; }
  .port-fields { display: flex; flex-wrap: wrap; align-items: end; gap: 8px; }
  .port-fields label { display: grid; gap: 4px; font-size: 0.875rem; }
  .port-fields input { width: 110px; padding: 8px; border: 1px solid var(--line); border-radius: 8px; background: var(--surface-2); color: var(--text); font: inherit; }

  p {
    margin: 8px 0 0;
  }
</style>
