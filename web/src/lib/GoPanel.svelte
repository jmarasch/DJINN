<script lang="ts">
  // Admin > GO: turning DJINN GO on, where to open it, safety settings,
  // who's listening, and the sign-in log.
  import { onDestroy, onMount } from 'svelte'
  import { api } from './api'
  import { app, toast } from './state.svelte'
  import { locale, t } from './i18n.svelte'
  import ConvertPanel from './ConvertPanel.svelte'
  import SubmissionsPanel from './SubmissionsPanel.svelte'
  import type { GoKeepRules, GoStatus } from './types'

  let status = $state<GoStatus | null>(null)
  let form = $state({ encryption: true, keep: { sign_out: 'delete', away: 'lock', away_days: 30 } as GoKeepRules, server_name: '', discoverable: true, enabled: false, allow_remote: false, allow_plain_remote: false, https_hostname: '', upnp: false, trusted_proxies: '', admin_two_step: false })
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
  let lanIp = $derived(status?.addresses[0] ? new URL(status.addresses[0]).hostname : t('gp.the_computer'))

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
    const s = await api.goStatus()
    status = s
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
      toast(status.enabled ? (status.running ? t('gp.on_toast') : t('gp.no_start')) : t('common.saved'))
    } catch (e) {
      toast(e instanceof Error ? e.message : t('common.save_failed_short'), true)
      if (status) form = formFrom(status)
    } finally {
      busy = false
    }
  }

  const ago = (s: number) => (s < 15 ? t('gp.now') : t('gp.secs_ago', { n: s }))
</script>

{#if status}
  <section class="card">
    <h2>{t('admin.menu.go')}</h2>
    <p class="small muted">{t('gp.intro')}</p>
    <label class="switch big">
      <input type="checkbox" bind:checked={form.enabled} onchange={save} disabled={busy} />
      <span>{form.enabled ? t('gp.is_on') : t('gp.is_off')}</span>
    </label>
    {#if status.error}<p class="error">{status.error}</p>{/if}
    <label>{t('gp.server_name')}
      <input type="text" bind:value={form.server_name} placeholder={status.default_name} maxlength="40" onchange={save} disabled={busy} />
    </label>
    <p class="small muted">{t('gp.server_name_note')}</p>
    <label class="switch big">
      <input type="checkbox" bind:checked={form.discoverable} onchange={save} disabled={busy} />
      <span>{t('gp.discoverable')}
        <span class="small muted block">{t('gp.discoverable_note')}</span>
      </span>
    </label>
    {#if status.running}
      <div>
        <p class="small">{t('gp.open_at')}</p>
        <ul class="addresses">
          {#each status.addresses as url (url)}<li><a href={url} target="_blank" rel="noopener">{url}</a></li>{/each}
          {#if status.tailscale}<li><a href={status.tailscale} target="_blank" rel="noopener">{status.tailscale}</a> <span class="muted small">(Tailscale)</span></li>{/if}
        </ul>
      </div>
    {/if}
    <p class="small muted">
      {t('gp.who_1')} <a href="#/admin/users">{t('admin.menu.users')}</a>{t('gp.who_2')}
      <a href="https://github.com/jmarasch/DJINN/blob/main/docs/guide/djinn-go.md" target="_blank" rel="noopener">{t('gp.guide')}</a>
      {t('gp.tailscale')}
    </p>
  </section>

  {#if status.running}
    <section class="card">
      <h2>{t('gp.listening')}</h2>
      {#each status.listening as l, i (i)}
        <div class="row">
          <div class="grow"><b>{l.username}</b> <span class="muted">{l.title}</span></div>
          <span class="muted small">{ago(l.seconds_ago)}</span>
        </div>
      {:else}
        <p class="muted">{t('gp.nobody')}</p>
      {/each}
      <p class="small muted">
        {t('gp.sending', { mbps: status.mbps.toLocaleString(locale.code, { minimumFractionDigits: 1, maximumFractionDigits: 1 }) })}
      </p>
    </section>
  {/if}

  {#if status.android_app}
    <section class="card">
      <h2>{t('gp.android')}</h2>
      <p class="small muted">{t('gp.android_note')}</p>
      <!-- The QR code is an SVG DJINN made itself. -->
      <div class="app-qr">{@html status.android_app.qr_svg}</div>
      <p class="small"><a href={status.android_app.url}>{status.android_app.url}</a></p>
      <p class="small muted app-sum">{t('gp.app_version', { version: status.android_app.version, sum: status.android_app.sha256 })}</p>
    </section>
  {/if}

  <section class="card">
    <h2>{t('gp.keep')}</h2>
    <p class="small muted">{t('gp.keep_note')}</p>
    <label>{t('gp.sign_out')}
      <select bind:value={form.keep.sign_out} onchange={save} disabled={busy}>
        <option value="delete">{t('gp.delete_kept')}</option>
        <option value="keep">{t('gp.keep_locked')}</option>
      </select>
    </label>
    <label>{t('gp.away')}
      <select bind:value={form.keep.away} onchange={save} disabled={busy}>
        <option value="lock">{t('gp.lock')}</option>
        <option value="delete">{t('gp.delete')}</option>
        <option value="off">{t('gp.nothing')}</option>
      </select>
    </label>
    {#if form.keep.away !== 'off'}
      <label>{t('gp.after_days')}
        <input type="number" min="1" max="3650" bind:value={form.keep.away_days} onchange={save} disabled={busy} />
      </label>
    {/if}
    <p class="small muted">{t('gp.keep_rules_note')}</p>
  </section>

  <SubmissionsPanel />

  <ConvertPanel />

  <section class="card">
    <h2>{t('gp.safety')}</h2>
    <label class="switch big">
      <input type="checkbox" bind:checked={form.encryption} onchange={save} disabled={busy} />
      <span>{t('gp.encrypt')}
        <span class="small muted block">{t('gp.encrypt_note_1')} <code>https://</code>{t('gp.encrypt_note_2')}</span>
      </span>
    </label>
    {#if status.security_code}
      <p class="small">{t('gp.code')} <code>{status.security_code}</code></p>
      <p class="small muted">{t('gp.code_note')}</p>
    {/if}
    <label class="switch big">
      <input type="checkbox" bind:checked={form.allow_remote} onchange={save} disabled={busy} />
      <span>{t('gp.remote')}
        <span class="small muted block">{t('gp.remote_note')}</span>
      </span>
    </label>
    {#if form.allow_remote}
      <div class="remote-setup">
        <h3>{t('gp.forwarding')}</h3>
        <p class="small muted">{t('gp.forwarding_note_1', { port: status.port, target: `${lanIp}:${status.port}` })} <code>https://</code>{t('gp.forwarding_note_2')}</p>
        <label class="switch">
          <input type="checkbox" bind:checked={form.allow_plain_remote} onchange={save} disabled={busy} />
          <span>{t('gp.plain')}
            <span class="small muted block">{t('gp.plain_note')}</span>
          </span>
        </label>
        <h3>{t('gp.https')}</h3>
        <p class="small muted">{t('gp.https_note')}</p>
        <label>{t('gp.hostname')} <input type="text" bind:value={form.https_hostname} placeholder="music.example.com" spellcheck="false" /></label>
        {#if form.https_hostname}
          <pre class="example">{caddyfile}</pre>
        {/if}
        <p class="small muted">{t('gp.dns_note_1')} <code>127.0.0.1</code> {t('gp.dns_note_2')}</p>
        <button class="small-button" onclick={save} disabled={busy}>{t('gp.save_https')}</button>
        <label class="switch big">
          <input type="checkbox" bind:checked={form.upnp} onchange={save} disabled={busy || !form.enabled || !form.https_hostname} />
          <span>{t('gp.upnp')}
            <span class="small muted block">{t('gp.upnp_note')}</span>
          </span>
        </label>
        {#if status.upnp_error}<p class="error">{status.upnp_error}</p>{/if}
        {#if form.upnp && !status.upnp_error && !status.upnp_verified}<p class="small muted">{t('gp.upnp_opening')}</p>{/if}
        {#if status.upnp_verified && status.upnp_external_ip}<p class="small muted">{t('gp.upnp_active', { ip: status.upnp_external_ip })}</p>{/if}
        {#if !form.upnp && form.https_hostname}
          <p class="small muted">{t('gp.no_upnp', { ip: lanIp })}</p>
        {/if}
      </div>
    {/if}
    <label class="switch big">
      <input type="checkbox" bind:checked={form.admin_two_step} onchange={save} disabled={busy || (!app.me?.two_step && !form.admin_two_step)} />
      <span>
        {t('gp.admin_two_step')}
        {#if !app.me?.two_step}<span class="small muted block">{t('gp.set_up_first')} <a href="#/account">{t('gp.your_account')}</a>{t('gp.first')}</span>{/if}
      </span>
    </label>
    <label>
      {t('gp.proxies')}
      <textarea rows="2" bind:value={form.trusted_proxies} placeholder="127.0.0.1" spellcheck="false"></textarea>
    </label>
    <p class="small muted">
      {t('gp.proxies_note')}
    </p>
    <button class="small-button" onclick={save} disabled={busy}>{t('gp.save_proxies')}</button>
  </section>

  <p class="small muted">
    {t('gp.signins_note')} <a href="#/admin/signins">{t('admin.group.activity')} &gt; {t('admin.menu.signins')}</a>.
  </p>
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
