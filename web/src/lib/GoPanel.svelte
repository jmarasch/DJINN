<script lang="ts">
  // Admin > GO: turning DJINN GO on, where to open it, safety settings,
  // who's listening, and the sign-in log.
  import { onDestroy, onMount } from 'svelte'
  import { api } from './api'
  import { deviceName } from './format'
  import { app, toast } from './state.svelte'
  import ConvertPanel from './ConvertPanel.svelte'
  import SubmissionsPanel from './SubmissionsPanel.svelte'
  import type { GoKeepRules, GoStatus, SignIn } from './types'

  let status = $state<GoStatus | null>(null)
  let form = $state({ encryption: true, keep: { sign_out: 'delete', away: 'lock', away_days: 30 } as GoKeepRules, server_name: '', discoverable: true, enabled: false, allow_remote: false, allow_plain_remote: false, https_hostname: '', upnp: false, trusted_proxies: '', admin_two_step: false })
  let signIns = $state<SignIn[]>([])
  let busy = $state(false)
  let timer: ReturnType<typeof setInterval> | undefined
  let caddyfile = $derived(`${form.https_hostname} {
    tls {
        issuer acme {
            disable_http_challenge
        }
    }
    reverse_proxy 127.0.0.1:${status?.port ?? 8067}
}`)
  let lanIp = $derived(status?.addresses[0] ? new URL(status.addresses[0]).hostname : 'the DJINN computer')

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
    form = formFrom(s)
  }

  function formFrom(s: GoStatus) {
    return { encryption: s.encryption, keep: { ...s.keep }, server_name: s.server_name, discoverable: s.discoverable, enabled: s.enabled, allow_remote: s.allow_remote, allow_plain_remote: s.allow_plain_remote, https_hostname: s.https_hostname, upnp: s.upnp, trusted_proxies: s.trusted_proxies, admin_two_step: s.admin_two_step }
  }

  async function save() {
    busy = true
    try {
      status = await api.saveGoSettings(form)
      form = formFrom(status)
      toast(status.enabled ? (status.running ? 'DJINN GO is on' : 'Saved, but GO could not start') : 'Saved')
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not save', true)
      if (status) form = formFrom(status)
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
    <label>Server name
      <input type="text" bind:value={form.server_name} placeholder={status.default_name} maxlength="40" onchange={save} disabled={busy} />
    </label>
    <p class="small muted">What people see when choosing a server, which helps on a network with more than one.</p>
    <label class="switch big">
      <input type="checkbox" bind:checked={form.discoverable} onchange={save} disabled={busy} />
      <span>Let phones on this network find this server
        <span class="small muted block">The app lists it by name, and anyone on the network can see that name and address. People still have to sign in.</span>
      </span>
    </label>
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

  {#if status.android_app}
    <section class="card">
      <h2>Android app</h2>
      <p class="small muted">This server carries the app, so people can install it without a store. They can scan this with a phone's camera, or find it when they open GO on Android. Android asks them to allow the install, and the app offers its own updates after that.</p>
      <!-- The QR code is an SVG DJINN made itself. -->
      <div class="app-qr">{@html status.android_app.qr_svg}</div>
      <p class="small"><a href={status.android_app.url}>{status.android_app.url}</a></p>
      <p class="small muted app-sum">Version {status.android_app.version}. SHA-256: {status.android_app.sha256}</p>
    </section>
  {/if}

  <section class="card">
    <h2>Songs kept on devices</h2>
    <p class="small muted">For people whose role can download. The app follows these rules, and learns them each time it reaches this server. A locked song stays on the device but won't play until it's unlocked.</p>
    <label>When someone signs out
      <select bind:value={form.keep.sign_out} onchange={save} disabled={busy}>
        <option value="delete">Delete their kept songs</option>
        <option value="keep">Keep them, locked until they sign back in</option>
      </select>
    </label>
    <label>When a device hasn't reached this server for a while
      <select bind:value={form.keep.away} onchange={save} disabled={busy}>
        <option value="lock">Lock kept songs until it reconnects</option>
        <option value="delete">Delete kept songs</option>
        <option value="off">Do nothing: they keep playing</option>
      </select>
    </label>
    {#if form.keep.away !== 'off'}
      <label>After this many days
        <input type="number" min="1" max="3650" bind:value={form.keep.away_days} onchange={save} disabled={busy} />
      </label>
    {/if}
    <p class="small muted">These rules only apply to songs kept from this server. Songs belong to one account on one server, so moving to another server, or someone else signing in on the device, leaves them alone. If a role loses its download permission, its songs from this server are deleted.</p>
  </section>

  <SubmissionsPanel />

  <ConvertPanel />

  <section class="card">
    <h2>Safety</h2>
    <label class="switch big">
      <input type="checkbox" bind:checked={form.encryption} onchange={save} disabled={busy} />
      <span>Encrypt connections
        <span class="small muted block">On by default, with nothing to set up: DJINN makes its own certificate. The Android app always uses it and checks it's still the same one. Browsers can use it too by opening an address with <code>https://</code>, but they warn that it isn't from a known authority. Plain addresses keep working, for cast devices and phone apps.</span>
      </span>
    </label>
    {#if status.security_code}
      <p class="small">Security code: <code>{status.security_code}</code></p>
      <p class="small muted">The app shows this code too when connecting. If they match, it's talking to this server. Scanning the QR code in GO checks this for you.</p>
    {/if}
    <label class="switch big">
      <input type="checkbox" bind:checked={form.allow_remote} onchange={save} disabled={busy} />
      <span>Allow connections from outside the home network
        <span class="small muted block">Off by default. Home network and Tailscale connections still work. Each role also needs its own remote GO permission.</span>
      </span>
    </label>
    {#if form.allow_remote}
      <div class="remote-setup">
        <h3>Router port forwarding</h3>
        <p class="small muted">To connect directly, forward TCP {status.port} on your router to {lanIp}:{status.port}. DJINN GO's app encrypts this by itself. Browsers warn about its certificate if you open <code>https://</code>, and other Subsonic apps need to be told to accept it.</p>
        <label class="switch">
          <input type="checkbox" bind:checked={form.allow_plain_remote} onchange={save} disabled={busy} />
          <span>Also allow unencrypted connections from outside
            <span class="small muted block">Off by default. With this on, signing in over plain HTTP works from anywhere, and passwords, app passwords, and music can be read by anyone along the way.</span>
          </span>
        </label>
        <h3>Recommended: HTTPS</h3>
        <p class="small muted">Give GO a public DNS name and run an HTTPS reverse proxy on this computer. With Caddy, add this site to its Caddyfile:</p>
        <label>Public hostname <input type="text" bind:value={form.https_hostname} placeholder="music.example.com" spellcheck="false" /></label>
        {#if form.https_hostname}
          <pre class="example">{caddyfile}</pre>
        {/if}
        <p class="small muted">Point the DNS name at your public IP. Set Trusted proxies below to <code>127.0.0.1</code> so GO can see the visitor's address and recognize HTTPS. Caddy obtains and renews the certificate.</p>
        <button class="small-button" onclick={save} disabled={busy}>Save HTTPS setup</button>
        <label class="switch big">
          <input type="checkbox" bind:checked={form.upnp} onchange={save} disabled={busy || !form.enabled || !form.https_hostname} />
          <span>Use UPnP to forward HTTPS on the router
            <span class="small muted block">Requires the HTTPS setup above. Maps TCP 443 to this computer for one hour at a time and renews it while GO is on. UPnP does not forward GO's HTTP port.</span>
          </span>
        </label>
        {#if status.upnp_error}<p class="error">{status.upnp_error}</p>{/if}
        {#if form.upnp && !status.upnp_error && !status.upnp_verified}<p class="small muted">Opening the router port and checking the HTTPS certificate...</p>{/if}
        {#if status.upnp_verified && status.upnp_external_ip}<p class="small muted">HTTPS mapping active at {status.upnp_external_ip}:443.</p>{/if}
        {#if !form.upnp && form.https_hostname}
          <p class="small muted">For HTTPS without UPnP, forward TCP 443 on your router to {lanIp}:443.</p>
        {/if}
      </div>
    {/if}
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

  .remote-setup { display: grid; gap: 10px; margin: 12px 0; }
  .remote-setup h3 { margin: 0; }
  .remote-setup label:not(.switch) { display: grid; gap: 4px; }
  .remote-setup input[type='text'] { padding: 8px 12px; border: 1px solid var(--line); border-radius: 10px; background: var(--surface-2); color: var(--text); font: inherit; }
  .example { margin: 0; padding: 12px; border-radius: 8px; background: var(--bg); white-space: pre-wrap; }

  .app-qr {
    width: 200px;
    padding: 8px;
    border-radius: 8px;
    background: #fff;
  }

  .app-qr :global(svg) {
    display: block;
    width: 100%;
    height: auto;
  }

  .app-sum {
    overflow-wrap: anywhere;
  }
</style>
