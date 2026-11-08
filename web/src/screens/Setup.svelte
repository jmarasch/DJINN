<script lang="ts">
  import { onMount } from 'svelte'
  import { api } from '../lib/api'
  import { app, connect } from '../lib/state.svelte'

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
            location.href = '/node'
            return
          }
        } catch {
          // Still starting.
        }
        setTimeout(wait, 1500)
      }
      setTimeout(wait, 2000)
    } catch (err) {
      error = err instanceof Error ? err.message : 'Setup failed'
      busy = false
    }
  }

  onMount(() => {
    void api.bootstrapStatus().then((status) => {
      needed = status.needed
      open = status.open ?? false
      address = status.address ?? null
    })
    if (!onScreen) return
    // Set up from a phone: follow along on the screen.
    const timer = setInterval(async () => {
      try {
        const node = await fetch('/api/node/status')
        if (node.ok && node.headers.get('content-type')?.includes('json')) {
          location.replace('/node?screen=1')
          return
        }
        if (!(await api.bootstrapStatus()).needed) location.replace('/tv')
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
      error = 'Passwords do not match'
      return
    }
    busy = true
    error = ''
    try {
      app.me = await api.bootstrap(token, username, password)
      app.setupLibrary = true
      connect()
    } catch (err) {
      error = err instanceof Error ? err.message : 'Setup failed'
    } finally {
      busy = false
    }
  }
</script>

{#if needed === false}
  <div class="card">
    <h1>Already set up</h1>
    <p class="muted">An admin account exists. Sign in instead.</p>
    <a class="button primary" href="/" onclick={finish}>Go to sign in</a>
  </div>
{:else if becomingNode}
  <div class="card">
    <h1>Starting as a node</h1>
    <p class="muted">
      <span class="spinner" aria-hidden="true"></span> DJINN is starting again as a node. This page moves on by itself.
      If it doesn't within a minute, start DJINN again and open this address.
    </p>
  </div>
{:else if needed && kind === 'node'}
  <div class="card">
    <h1>Set up as a node</h1>
    <p class="muted">
      A node plays one room's music on this computer's speakers, in step with your main DJINN, and takes everything
      from it: no library here, and phones keep using the main DJINN. Next you'll enter the main DJINN's address and
      accept this node there with your password.
    </p>
    {#if !token && !open}
      <p class="error">This page needs the setup link from the server log.</p>
    {/if}
    {#if error}<p class="error">{error}</p>{/if}
    <div class="choices">
      <button class="primary" disabled={busy || (!token && !open)} onclick={becomeNode}>Make this a node</button>
      <button disabled={busy} onclick={() => (kind = null)}>Back</button>
    </div>
  </div>
{:else if needed && kind === null}
  <div class="card">
    <h1>Welcome to DJINN</h1>
    <p class="muted">What will this computer be?</p>
    <div class="kinds">
      <button class="primary" onclick={() => (kind = 'main')}>
        <b>Main DJINN</b><br /><span class="small">Holds the music library and runs the party.</span>
      </button>
      <button class="primary" onclick={() => (kind = 'node')}>
        <b>Node</b><br /><span class="small">Plays a room for a main DJINN you already have.</span>
      </button>
    </div>
    {#if onScreen && address}
      <p class="muted">No keyboard here? Open <b>{address}/setup</b> on a phone or computer on this network.</p>
    {/if}
  </div>
{:else if needed}
  <form class="card" onsubmit={submit}>
    <h1>Welcome to DJINN</h1>
    <p class="muted">Create the admin account. You can add guests afterwards.</p>
    {#if !token && !open}
      <p class="error">This page needs the setup link from the server log.</p>
    {/if}
    <label>
      Username
      <input bind:value={username} autocomplete="username" autocapitalize="none" required />
    </label>
    <label>
      Password
      <input type="password" bind:value={password} autocomplete="new-password" minlength="8" required />
    </label>
    <label>
      Confirm password
      <input type="password" bind:value={confirm} autocomplete="new-password" required />
    </label>
    {#if error}<p class="error">{error}</p>{/if}
    <button class="primary" disabled={busy || (!token && !open)}>{busy ? 'Creating...' : 'Create admin'}</button>
    <button type="button" class="link" onclick={() => (kind = null)}>Back</button>
  </form>
{/if}

<style>
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
