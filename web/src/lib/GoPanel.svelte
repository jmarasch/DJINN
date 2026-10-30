<script lang="ts">
  // Admin > GO: turning DJINN GO on, where to open it, safety settings,
  // who's listening, and the sign-in log.
  import { onDestroy, onMount } from 'svelte'
  import { api } from './api'
  import { deviceName } from './format'
  import { app, toast } from './state.svelte'
  import ConvertPanel from './ConvertPanel.svelte'
  import SubmissionsPanel from './SubmissionsPanel.svelte'
  import type { GoStatus, SignIn } from './types'

  let status = $state<GoStatus | null>(null)
  let form = $state({ enabled: false, trusted_proxies: '', admin_two_step: false })
  let signIns = $state<SignIn[]>([])
  let busy = $state(false)
  let timer: ReturnType<typeof setInterval> | undefined

  onMount(() => {
    load().catch((e) => toast(e.message, true))
    // Keep "listening now" fresh while the tab is open.
    timer = setInterval(() => {
      api
        .goStatus()
        .then((s) => (status = s))
        .catch(() => {})
    }, 10_000)
  })
  onDestroy(() => clearInterval(timer))

  async function load() {
    const [s, log] = await Promise.all([api.goStatus(), api.signIns()])
    status = s
    signIns = log
    form = { enabled: s.enabled, trusted_proxies: s.trusted_proxies, admin_two_step: s.admin_two_step }
  }

  async function save() {
    busy = true
    try {
      status = await api.saveGoSettings(form)
      toast(status.enabled ? (status.running ? 'DJINN GO is on' : 'Saved, but GO could not start') : 'Saved')
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not save', true)
      if (status) form = { enabled: status.enabled, trusted_proxies: status.trusted_proxies, admin_two_step: status.admin_two_step }
    } finally {
      busy = false
    }
  }

  const outcomes: Record<string, string> = {
    ok: 'Signed in',
    wrong_password: 'Wrong password',
    locked_out: 'Locked out (too many tries)',
    waiting: 'Waiting for approval',
    turned_off: 'Account turned off',
    wrong_code: 'Wrong two-step code',
    no_go: "Role can't use GO",
    not_home: 'Away from home, not allowed',
  }
  const when = (ms: number) =>
    new Date(ms).toLocaleString([], { month: 'short', day: 'numeric', hour: 'numeric', minute: '2-digit' })
  const ago = (s: number) => (s < 15 ? 'now' : `${s} s ago`)
</script>

{#if status}
  <section class="card">
    <h2>DJINN GO</h2>
    <p class="small muted">Personal listening, apart from the party, on its own address. Each person streams songs to their own phone or computer, which does the playing.</p>
    <label class="switch big">
      <input type="checkbox" bind:checked={form.enabled} onchange={save} disabled={busy} />
      <span>DJINN GO is {form.enabled ? 'on' : 'off'}</span>
    </label>
    {#if status.error}<p class="error">{status.error}</p>{/if}
    {#if status.running}
      <div>
        <p class="small">On your home network, open:</p>
        <ul class="addresses">
          {#each status.addresses as url (url)}<li><a href={url} target="_blank" rel="noopener">{url}</a></li>{/each}
          {#if status.tailscale}<li><a href={status.tailscale} target="_blank" rel="noopener">{status.tailscale}</a> <span class="muted small">(Tailscale)</span></li>{/if}
        </ul>
      </div>
    {/if}
    <p class="small muted">
      Who can use it, and from where, is set per role under <a href="#/admin/users">Users</a>. To use it away from home, see
      <a href="https://github.com/jmarasch/DJINN/blob/main/docs/guide/djinn-go.md" target="_blank" rel="noopener">DJINN GO in the guide</a>
      (Tailscale is the easiest and safest).
    </p>
  </section>

  {#if status.running}
    <section class="card">
      <h2>Listening now</h2>
      {#each status.listening as l, i (i)}
        <div class="row">
          <div class="grow"><b>{l.username}</b> <span class="muted">{l.title}</span></div>
          <span class="muted small">{ago(l.seconds_ago)}</span>
        </div>
      {:else}
        <p class="muted">Nobody right now.</p>
      {/each}
      <p class="small muted">
        Sending {status.mbps.toFixed(1)} Mbit/s. Each listener takes about 1 Mbit/s for FLAC or 0.3 for MP3 of your internet upload when they're away from home.
      </p>
    </section>
  {/if}

  <SubmissionsPanel />

  <ConvertPanel />

  <section class="card">
    <h2>Safety</h2>
    <label class="switch big">
      <input type="checkbox" bind:checked={form.admin_two_step} onchange={save} disabled={busy || (!app.me?.two_step && !form.admin_two_step)} />
      <span>
        Admins must use two-step sign-in for GO
        {#if !app.me?.two_step}<span class="small muted block">Set it up for <a href="#/account">your own account</a> first.</span>{/if}
      </span>
    </label>
    <label>
      Trusted proxies
      <textarea rows="2" bind:value={form.trusted_proxies} placeholder="127.0.0.1" spellcheck="false"></textarea>
    </label>
    <p class="small muted">
      Only for Cloudflare Tunnel or a reverse proxy in front of GO: its address, so DJINN sees each visitor's real address (for lockouts and the home-network check). For cloudflared on this computer, that's 127.0.0.1. Leave empty otherwise.
    </p>
    <button class="small-button" onclick={save} disabled={busy}>Save proxies</button>
  </section>

  <section class="card">
    <h2>Sign-in log</h2>
    <p class="small muted">Every sign-in to DJINN and DJINN GO, kept for 90 days. Five wrong passwords lock an account for 15 minutes.</p>
    {#each signIns as s (s.id)}
      <div class="row" class:bad={s.outcome !== 'ok'}>
        <div class="grow">
          <div><b>{s.username}</b> {outcomes[s.outcome] ?? s.outcome} <span class="muted small">{s.via === 'go' ? 'GO' : 'party'}</span></div>
          <div class="muted small">{when(s.at)}, {s.ip ?? 'unknown address'}, {deviceName(s.user_agent)}</div>
        </div>
      </div>
    {:else}
      <p class="muted">No sign-ins yet.</p>
    {/each}
  </section>
{/if}

<style>
  .addresses {
    margin: 4px 0 0;
    padding-left: 18px;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 6px 0;
    border-bottom: 1px solid var(--line);
  }

  .row.bad b {
    color: var(--bad);
  }

  .grow {
    flex: 1;
    min-width: 0;
  }

  .switch.big {
    align-items: flex-start;
  }

  .block {
    display: block;
  }

  textarea {
    padding: 8px 12px;
    border: 1px solid var(--line);
    border-radius: 10px;
    background: var(--surface-2);
    color: var(--text);
    font: inherit;
    font-size: 0.875rem;
    resize: vertical;
  }

  p {
    margin: 0;
  }

  .small-button {
    align-self: flex-start;
  }
</style>
