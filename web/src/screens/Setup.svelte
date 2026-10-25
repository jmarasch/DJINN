<script lang="ts">
  import { onMount } from 'svelte'
  import { api } from '../lib/api'
  import { app, connect } from '../lib/state.svelte'

  const token = new URLSearchParams(location.search).get('token') ?? ''
  let needed = $state<boolean | null>(null)
  let username = $state('')
  let password = $state('')
  let confirm = $state('')
  let error = $state('')
  let busy = $state(false)

  onMount(async () => {
    needed = (await api.bootstrapStatus()).needed
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
{:else if needed}
  <form class="card" onsubmit={submit}>
    <h1>Welcome to DJINN</h1>
    <p class="muted">Create the admin account. You can add guests afterwards.</p>
    {#if !token}
      <p class="error">This page needs the setup link from the server log.</p>
    {/if}
    <label>
      Username
      <input bind:value={username} autocomplete="username" autocapitalize="none" required />
    </label>
    <label>
      Password
      <input type="password" bind:value={password} autocomplete="new-password" minlength="6" required />
    </label>
    <label>
      Confirm password
      <input type="password" bind:value={confirm} autocomplete="new-password" required />
    </label>
    {#if error}<p class="error">{error}</p>{/if}
    <button class="primary" disabled={busy || !token}>{busy ? 'Creating...' : 'Create admin'}</button>
  </form>
{/if}
