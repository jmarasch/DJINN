<script lang="ts">
  import { onMount } from 'svelte'
  import { api } from '../lib/api'
  import { app, connect } from '../lib/state.svelte'
  import { t } from '../lib/i18n.svelte'

  const token = new URLSearchParams(location.search).get('token') ?? ''
  let needed = $state<boolean | null>(null)
  /** A box without a screen: no setup link needed yet. */
  let open = $state(false)
  let username = $state('')
  let password = $state('')
  let confirm = $state('')
  let error = $state('')
  let busy = $state(false)
  /** Main (the full DJINN) or a node (plays a room for a main DJINN); asked first. */
  let kind = $state<'main' | 'node' | null>(null)
  /** On the box's own screen (the kiosk opens /screen). */
  const onScreen = new URLSearchParams(location.search).has('screen')
  let address = $state<string | null>(null)
  /** Main DJINNs heard on this network. */
  let mains = $state<{ name: string; address: string; joining: boolean }[]>([])
  /** Said this box is its own main DJINN, not a node for what was found. */
  let notNode = $state(false)
  let becomingNode = $state(false)

  async function becomeNode() {
    busy = true
    error = ''
    try {
      await api.bootstrapNode(token)
      becomingNode = true
      // DJINN starts again as a node; its page appears once it's back.
      const wait = async () => {
        try {
          const res = await fetch('/api/node/status')
          if (res.ok) {
            location.href = onScreen ? '/node?screen=1' : '/node'
            return
          }
        } catch {
          // Still starting.
        }
        setTimeout(wait, 1500)
      }
      setTimeout(wait, 2000)
    } catch (err) {
      error = err instanceof Error ? err.message : t('setup.failed')
      busy = false
    }
  }

  onMount(() => {
    void api.bootstrapStatus().then((status) => {
      needed = status.needed
      open = status.open ?? false
      address = status.address ?? null
      mains = status.mains ?? []
    })
    const timer = setInterval(async () => {
      try {
        if (onScreen) {
          // Set up from a phone: follow along on the screen.
          const node = await fetch('/api/node/status')
          if (node.ok && node.headers.get('content-type')?.includes('json')) {
            location.replace('/node?screen=1')
            return
          }
        }
        const status = await api.bootstrapStatus()
        mains = status.mains ?? []
        if (onScreen && !status.needed) location.replace('/tv')
      } catch {
        // Restarting; the next look finds out.
      }
    }, 3000)
    return () => clearInterval(timer)
  })

  function finish() {
    history.replaceState(null, '', '/')
  }

  async function submit(e: SubmitEvent) {
    e.preventDefault()
    if (password !== confirm) {
      error = t('setup.no_match')
      return
    }
    busy = true
    error = ''
    try {
      app.me = await api.bootstrap(token, username, password)
      app.setupLibrary = true
      connect()
    } catch (err) {
      error = err instanceof Error ? err.message : t('setup.failed')
    } finally {
      busy = false
    }
  }
</script>

{#if needed === false}
  <div class="card">
    <h1>{t('setup.done')}</h1>
    <p class="muted">{t('setup.done_note')}</p>
    <a class="button primary" href="/" onclick={finish}>{t('setup.to_sign_in')}</a>
  </div>
{:else if becomingNode}
  <div class="card">
    <h1>{t('setup.starting_node')}</h1>
    <p class="muted">
      <span class="spinner" aria-hidden="true"></span> {t('setup.starting_node_note')}
    </p>
  </div>
{:else if needed && kind === 'node'}
  <div class="card">
    <h1>{t('setup.as_node')}</h1>
    <p class="muted">
      {t('setup.as_node_note')}
    </p>
    {#if !token && !open}
      <p class="error">{t('setup.needs_link')}</p>
    {/if}
    {#if error}<p class="error">{error}</p>{/if}
    <div class="choices">
      <button class="primary" disabled={busy || (!token && !open)} onclick={becomeNode}>{t('setup.make_node')}</button>
      <button disabled={busy} onclick={() => (kind = null)}>{t('setup.back')}</button>
    </div>
  </div>
{:else if needed && kind === null && mains.length && !notNode}
  <div class="card">
    <h1>{t('setup.is_node')}</h1>
    <p>
      {mains.length === 1 ? t('setup.found_one') : t('setup.found_many')}
      <b>{mains.map((m) => m.name).join(', ')}</b>.
    </p>
    <p class="muted">{t('setup.node_is')}</p>
    <div class="kinds">
      <button class="primary" onclick={() => (kind = 'node')}>{t('setup.yes_node')}</button>
      <button onclick={() => (notNode = true)}>{t('setup.no_main')}</button>
    </div>
  </div>
{:else if needed && kind === null}
  <div class="card">
    <img class="shiny" src="/images/logo.webp" alt="" />
    <h1>{t('setup.welcome')}</h1>
    <p class="muted">{t('setup.what')}</p>
    <div class="kinds">
      <button class="primary" onclick={() => (kind = 'main')}>
        <b>{t('setup.main')}</b><br /><span class="small">{t('setup.main_note')}</span>
      </button>
      <button class="primary" onclick={() => (kind = 'node')}>
        <b>{t('setup.node')}</b><br /><span class="small">{t('setup.node_note')}</span>
      </button>
    </div>
    {#if onScreen && address}
      <p class="muted">{t('setup.no_keyboard', { address: `${address}/setup` })}</p>
    {/if}
  </div>
{:else if needed}
  <form class="card" onsubmit={submit}>
    <h1>{t('setup.welcome')}</h1>
    <p class="muted">{t('setup.create_note')}</p>
    {#if !token && !open}
      <p class="error">{t('setup.needs_link')}</p>
    {/if}
    <label>
      {t('setup.username')}
      <input bind:value={username} autocomplete="username" autocapitalize="none" required />
    </label>
    <label>
      {t('setup.password')}
      <input type="password" bind:value={password} autocomplete="new-password" minlength="8" required />
    </label>
    <label>
      {t('setup.confirm')}
      <input type="password" bind:value={confirm} autocomplete="new-password" required />
    </label>
    {#if error}<p class="error">{error}</p>{/if}
    <button class="primary" disabled={busy || (!token && !open)}>{busy ? t('setup.creating') : t('setup.create')}</button>
    <button type="button" class="link" onclick={() => (kind = null)}>{t('setup.back')}</button>
  </form>
{/if}

<style>
  .shiny {
    align-self: center;
    width: 112px;
    height: 112px;
  }

  .kinds {
    display: flex;
    flex-direction: column;
    gap: 10px;
    margin-bottom: 12px;
  }

  .choices {
    display: flex;
    gap: 8px;
  }

  .link {
    background: transparent;
    color: var(--muted);
    text-decoration: underline;
  }
</style>
