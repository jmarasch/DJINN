<script lang="ts">
  // Admin > System: how DJINN is doing, and restarting or quitting it.
  import { onMount } from 'svelte'
  import { api } from './api'
  import { reloadAfterRestart } from './restart'
  import { toast } from './state.svelte'
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
      if (!leaving) toast(e instanceof Error ? e.message : 'Could not load the status', true)
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
      toast('Ports saved. Restart DJINN to apply them.')
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not save ports', true)
    } finally {
      savingPorts = false
    }
  }

  async function restart() {
    if (!confirm('Restart DJINN? The music stops for a few seconds, then carries on where it was.')) return
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
      toast(e instanceof Error ? e.message : 'Could not restart', true)
    }
  }

  async function quit() {
    if (!confirm('Quit DJINN? The music stops, and nobody can use DJINN until it is started again on its computer.'))
      return
    try {
      await api.quit()
      leaving = 'quit'
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not quit', true)
    }
  }

  const size = (bytes: number) =>
    bytes >= 1e9 ? `${(bytes / 1e9).toFixed(1)} GB` : `${(bytes / 1e6).toFixed(bytes >= 1e7 ? 0 : 1)} MB`

  function uptime(since: number): string {
    const minutes = Math.floor((Date.now() - since) / 60_000)
    if (minutes < 1) return 'less than a minute'
    if (minutes < 60) return `${minutes} min`
    const hours = Math.floor(minutes / 60)
    if (hours < 48) return `${hours} h ${minutes % 60} min`
    return `${Math.floor(hours / 24)} days`
  }

  const stateLabel = { idle: 'Nothing playing', playing: 'Playing', paused: 'Paused' }
</script>

<section class="card">
  <h2>Status</h2>
  {#if leaving === 'restart'}
    <p class="small"><span class="spinner" aria-hidden="true"></span> Restarting. This page reloads when DJINN is back.</p>
  {:else if leaving === 'quit'}
    <p class="small">DJINN has quit. Start it again on its computer to carry on.</p>
  {:else if status}
    <dl class="facts small">
      <dt>DJINN</dt>
      <dd>
        {status.version}{#if status.started_ms}, running for {uptime(status.started_ms)}{/if}
      </dd>

      <dt>Phones open</dt>
      <dd>
        {#each status.addresses as address (address)}
          <div><a href={address} target="_blank" rel="noopener noreferrer">{address}</a></div>
        {:else}
          <span class="muted">Unknown</span>
        {/each}
        {#if status.public_network}
          <div class="bad">Windows has this network set to Public, which keeps phones out.</div>
        {/if}
      </dd>

      <dt>Library</dt>
      <dd>
        {status.songs.toLocaleString()} songs in {status.library_folders}
        {status.library_folders === 1 ? 'folder' : 'folders'}{#if status.scanning}, checking for changes...{/if}
      </dd>

      <dt>Music</dt>
      <dd>{stateLabel[status.state]}</dd>

      <dt>Speakers</dt>
      <dd>
        {#each status.outputs.outputs.filter((o) => o.state !== 'disabled') as output (output.id)}
          <div>
            {output.device ?? 'Waiting for a device'}{#if output.state === 'waiting'}
              <span class="muted"> (waiting)</span>{/if}
            {#if output.error}<span class="bad"> {output.error}</span>{/if}
          </div>
        {:else}
          <span class="muted">None on</span>
        {/each}
        {#if status.outputs.error}<div class="bad">{status.outputs.error}</div>{/if}
      </dd>

      <dt>Casting to</dt>
      <dd>
        {#each status.casts as cast (cast.name)}
          <div>
            {cast.name}{#if cast.error}<span class="bad"> {cast.error}</span>{:else if !cast.playing}<span class="muted">
                (connecting)</span
              >{/if}
          </div>
        {:else}
          <span class="muted">Nothing</span>
        {/each}
      </dd>

      <dt>AI</dt>
      <dd>
        Listening {status.ai_listening ? 'loaded' : 'off'}, reader {status.ai_reader ? 'loaded' : 'off'}
      </dd>

      <dt>Storage</dt>
      <dd>
        Database {size(status.database_bytes)}, artwork {size(status.art_bytes)}, AI models
        {size(status.models_bytes)}
        {#if status.data_dir}<div class="muted path">{status.data_dir}</div>{/if}
      </dd>

      {#if status.log_file}
        <dt>Log file</dt>
        <dd class="muted path">{status.log_file}</dd>
      {/if}
    </dl>

    {#if ports}
      <div class="port-settings">
        <h3>Server ports</h3>
        <div class="port-fields">
          <label>Party
            <input type="number" min="1" max="65535" step="1" bind:value={portForm.party_port} />
          </label>
          <label>DJINN GO
            <input type="number" min="1" max="65535" step="1" bind:value={portForm.go_port} />
          </label>
          <button onclick={savePorts} disabled={savingPorts || !Number.isInteger(portForm.party_port) || !Number.isInteger(portForm.go_port) || portForm.party_port < 1 || portForm.go_port < 1 || portForm.party_port > 65535 || portForm.go_port > 65535 || portForm.party_port === portForm.go_port}>Save ports</button>
        </div>
        <p class="small muted">Current: party {ports.party_port}, GO {ports.go_port}. Saved changes take effect after a restart. Update any router forwarding or reverse proxy to use the new ports.</p>
      </div>
    {/if}

    <div class="actions">
      <button onclick={restart}>Restart DJINN</button>
      <button class="danger" onclick={quit}>Quit DJINN</button>
    </div>
    <p class="small muted">
      Restarting picks the music up where it was. After quitting, DJINN has to be started again on its computer.
    </p>
  {:else}
    <p class="small muted">Loading...</p>
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
