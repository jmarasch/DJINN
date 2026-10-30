<script lang="ts">
  // Creating an account: from an invite link (/signup?invite=...), or from
  // the sign-in screen while sign-ups are open or reviewed.
  import { onMount } from 'svelte'
  import { api } from '../lib/api'
  import { app, connect } from '../lib/state.svelte'

  let { onback }: { onback?: () => void } = $props()

  const invite = new URLSearchParams(location.search).get('invite') ?? ''
  let inviteRole = $state<string | null>(null)
  let inviteError = $state('')
  let username = $state('')
  let password = $state('')
  let confirm = $state('')
  let error = $state('')
  let busy = $state(false)
  let waiting = $state(false)

  onMount(async () => {
    if (!invite) return
    try {
      inviteRole = (await api.checkInvite(invite)).role
    } catch (e) {
      inviteError = e instanceof Error ? e.message : 'This invite link does not work'
    }
  })

  function toSignIn() {
    if (invite) history.replaceState(null, '', '/')
    onback?.()
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
      const result = await api.signUp(username, password, invite || undefined)
      if ('pending' in result) {
        waiting = true
      } else {
        if (invite) history.replaceState(null, '', '/')
        app.me = result
        connect()
      }
    } catch (err) {
      error = err instanceof Error ? err.message : 'Could not create the account'
    } finally {
      busy = false
    }
  }
</script>

{#if waiting}
  <div class="card">
    <h1>Almost there</h1>
    <p class="muted">Your account is waiting for the host to approve it. Sign in once they have.</p>
    <button class="primary" onclick={toSignIn}>Go to sign in</button>
  </div>
{:else if inviteError}
  <div class="card">
    <h1>Invite link</h1>
    <p class="error">{inviteError}</p>
    <p class="muted">Ask the host for a new one.</p>
    <button onclick={toSignIn}>Go to sign in</button>
  </div>
{:else}
  <form class="card" onsubmit={submit}>
    <h1>Create an account</h1>
    {#if invite}
      <p class="muted">
        {inviteRole ? `You're invited to DJINN as a ${inviteRole}.` : 'Checking your invite...'}
      </p>
    {:else if app.mode?.signup === 'review'}
      <p class="muted">The host approves new accounts before they can sign in.</p>
    {/if}
    <label>
      Username
      <input bind:value={username} autocomplete="username" autocapitalize="none" maxlength="32" required />
    </label>
    <label>
      Password
      <input type="password" bind:value={password} autocomplete="new-password" minlength="8" required />
    </label>
    <label>
      Confirm password
      <input type="password" bind:value={confirm} autocomplete="new-password" required />
    </label>
    <p class="small muted">At least 8 characters. A few words together are easy to remember and hard to guess.</p>
    {#if error}<p class="error">{error}</p>{/if}
    <button class="primary" disabled={busy || (!!invite && !inviteRole)}>
      {busy ? 'Creating...' : 'Create account'}
    </button>
    <button type="button" class="link" onclick={toSignIn}>I already have an account</button>
  </form>
{/if}

<style>
  .link {
    border: none;
    background: none;
    color: var(--muted);
    text-decoration: underline;
  }
</style>
