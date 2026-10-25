<script lang="ts">
  import { api } from '../lib/api'
  import { app, connect } from '../lib/state.svelte'

  let username = $state('')
  let password = $state('')
  let error = $state('')
  let busy = $state(false)

  async function submit(e: SubmitEvent) {
    e.preventDefault()
    busy = true
    error = ''
    try {
      app.me = await api.login(username, password)
      app.hostLogin = false
      connect()
    } catch (err) {
      error = err instanceof Error ? err.message : 'Login failed'
    } finally {
      busy = false
    }
  }
</script>

<form class="card" onsubmit={submit}>
  <h1>DJINN</h1>
  <p class="muted">{app.mode?.guest_mode ? 'Host sign in.' : 'Sign in to queue songs.'}</p>
  <label>
    Username
    <input bind:value={username} autocomplete="username" autocapitalize="none" required />
  </label>
  <label>
    Password
    <input type="password" bind:value={password} autocomplete="current-password" required />
  </label>
  {#if error}<p class="error">{error}</p>{/if}
  <button class="primary" disabled={busy}>{busy ? 'Signing in...' : 'Sign in'}</button>
</form>
