<script lang="ts">
  import { api } from '../lib/api'
  import { app, connect } from '../lib/state.svelte'
  import type { User } from '../lib/types'

  let {
    title = 'DJINN',
    subtitle,
    onsignedin,
  }: {
    title?: string
    subtitle?: string
    /** Instead of joining the party (DJINN GO doesn't). */
    onsignedin?: (me: User) => void
  } = $props()

  let username = $state('')
  let password = $state('')
  let code = $state('')
  /** Set once the password was right and a two-step code is needed. */
  let ticket = $state<string | null>(null)
  let error = $state('')
  let busy = $state(false)

  function signedIn(me: User) {
    app.me = me
    app.hostLogin = false
    if (onsignedin) onsignedin(me)
    else connect()
  }

  async function submit(e: SubmitEvent) {
    e.preventDefault()
    busy = true
    error = ''
    try {
      if (ticket) {
        signedIn(await api.finishTwoStep(ticket, code))
      } else {
        const result = await api.login(username, password)
        if ('ticket' in result) {
          ticket = result.ticket
          password = ''
        } else {
          signedIn(result)
        }
      }
    } catch (err) {
      error = err instanceof Error ? err.message : 'Login failed'
      // An expired or used-up code step starts over with the password.
      if (ticket && /again|too many/i.test(error)) {
        ticket = null
        code = ''
      }
    } finally {
      busy = false
    }
  }
</script>

<form class="card" onsubmit={submit}>
  <h1>{title}</h1>
  {#if ticket}
    <p class="muted">Type the 6-digit code from your authenticator app. Lost your phone? Use one of your recovery codes.</p>
    <label>
      Code
      <!-- svelte-ignore a11y_autofocus -->
      <input bind:value={code} inputmode="numeric" autocomplete="one-time-code" autofocus required />
    </label>
    {#if error}<p class="error">{error}</p>{/if}
    <button class="primary" disabled={busy}>{busy ? 'Checking...' : 'Sign in'}</button>
    <button type="button" class="link" onclick={() => ((ticket = null), (code = ''), (error = ''))}>Start over</button>
  {:else}
    <p class="muted">{subtitle ?? (app.mode?.guest_mode ? 'Host sign in.' : 'Sign in to queue songs.')}</p>
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
  {/if}
</form>

<style>
  .link {
    border: none;
    background: none;
    color: var(--muted);
    text-decoration: underline;
  }
</style>
