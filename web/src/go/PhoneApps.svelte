<script lang="ts">
  // Using DJINN GO from Subsonic phone apps: the address, your username,
  // and an app password just for them.
  import { t } from '../lib/i18n.svelte'
  import { onMount } from 'svelte'
  import { api } from '../lib/api'
  import { app, toast } from '../lib/state.svelte'

  let exists = $state(false)
  /** The new app password, shown only until this page closes. */
  let made = $state<string | null>(null)
  let shown = $state(false)
  let loaded = $state(false)
  /** Asking for the account's password before making one. */
  let asking = $state(false)
  let current = $state('')

  onMount(async () => {
    try {
      exists = (await api.appPassword()).exists
    } finally {
      loaded = true
    }
  })

  async function make(e: SubmitEvent) {
    e.preventDefault()
    try {
      made = (await api.newAppPassword(current)).password
      exists = true
      shown = true
      asking = false
      current = ''
    } catch (e) {
      toast(e instanceof Error ? e.message : t('pa2.make_failed'), true)
    }
  }

  async function remove() {
    if (!confirm(t('pa2.remove_q'))) return
    try {
      await api.removeAppPassword()
      exists = false
      made = null
    } catch (e) {
      toast(e instanceof Error ? e.message : t('common.remove_failed'), true)
    }
  }

  async function copy(text: string) {
    try {
      await navigator.clipboard.writeText(text)
      toast(t('pa2.copied'))
    } catch {
      toast(t('pa2.copy_by_hand'), true)
    }
  }
</script>

<section class="card">
  <h2>{t('pa2.title')}</h2>
  <p class="small muted">
    {t('pa2.intro')}
  </p>
  {#if loaded}
    <dl>
      <dt>{t('pa2.server')}</dt>
      <dd><span class="mono">{location.origin}</span> <button class="small-button" onclick={() => copy(location.origin)}>{t('pa2.copy')}</button></dd>
      <dt>{t('acc.username')}</dt>
      <dd class="mono">{app.me?.username}</dd>
      <dt>{t('acc.password')}</dt>
      <dd>
        {#if made}
          <span class="mono">{shown ? made : '••••••••••••••••••••'}</span>
          <button class="small-button" onclick={() => (shown = !shown)}>{shown ? t('pa2.hide') : t('pa2.show')}</button>
          <button class="small-button" onclick={() => made && copy(made)}>{t('pa2.copy')}</button>
        {:else if exists}
          <span class="muted">{t('pa2.have_one')}</span>
        {:else}
          <span class="muted">{t('pa2.make_first')}</span>
        {/if}
      </dd>
    </dl>
    <p class="small muted">
      {t('pa2.note')}
    </p>
    {#if made}<p class="small muted">{t('pa2.copy_now')}</p>{/if}
    {#if asking}
      <form class="buttons" onsubmit={make}>
        <input type="password" bind:value={current} autocomplete="current-password" placeholder={t('ts.your_password')} required aria-label={t('ts.your_password')} />
        <button class="small-button">{t('pa2.make_it')}</button>
        {#if exists}<span class="small muted">{t('pa2.old_stops')}</span>{/if}
      </form>
    {:else}
      <div class="buttons">
        <button class="small-button" onclick={() => (asking = true)}>{exists ? t('pa2.make_new') : t('pa2.make')}</button>
        {#if exists}<button class="small-button" onclick={remove}>{t('pa2.remove')}</button>{/if}
      </div>
    {/if}
  {/if}
</section>

<style>
  dl {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 8px 12px;
    align-items: center;
    margin: 0;
  }

  dt {
    color: var(--muted);
  }

  dd {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    margin: 0;
    min-width: 0;
  }

  .mono {
    font-family: ui-monospace, monospace;
    overflow-wrap: anywhere;
  }

  p {
    margin: 0;
  }

  .buttons {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
</style>
