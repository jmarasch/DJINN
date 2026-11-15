<script lang="ts">
  // The password (and, at the strictest level, a two-step code) that opens
  // the protected areas. In a sheet over Admin (UnlockSheet) or on the
  // library editor window's own lock screen.
  import { onMount } from 'svelte'
  import { api } from './api'
  import type { ProtectedStatus } from './types'

  let {
    onopened,
    note,
    skipIfOpen = true,
  }: {
    onopened: () => void
    /** What unlocking is for and when it locks again. */
    note: string
    /** Carries on at once if the areas are already open. */
    skipIfOpen?: boolean
  } = $props()

  let status = $state<ProtectedStatus | null>(null)
  let password = $state('')
  let code = $state('')
  let busy = $state(false)
  let error = $state('')

  onMount(() => {
    api
      .protectedStatus()
      .then((s) => {
        status = s
        if (skipIfOpen && s.open) onopened()
      })
      .catch((e) => (error = e instanceof Error ? e.message : 'Could not load'))
  })

  async function submit(e: SubmitEvent) {
    e.preventDefault()
    busy = true
    error = ''
    try {
      await api.protectedEnter(password, code)
      onopened()
    } catch (err) {
      error = err instanceof Error ? err.message : 'That did not work'
      code = ''
    } finally {
      busy = false
    }
  }
</script>

<form onsubmit={submit}>
  <p class="small muted">
    {note}
    Enter your password{status?.level === 'max' ? ' and a code from your authenticator app' : ''} to unlock it.
  </p>
  {#if status && !status.can_open}
    <p class="small bad">
      The strictest level needs two-step sign-in, and your account doesn't have it yet. Set it up on your Account page,
      then come back.
    </p>
  {/if}
  <label>
    Password
    <input type="password" autocomplete="current-password" bind:value={password} />
  </label>
  {#if status?.level === 'max'}
    <label>
      Code
      <input inputmode="numeric" autocomplete="one-time-code" placeholder="6 digits or a recovery code" bind:value={code} />
    </label>
  {/if}
  {#if error}<p class="small bad">{error}</p>{/if}
  <button class="primary" disabled={busy || !password || (status !== null && !status.can_open)}>Unlock</button>
</form>

<style>
  form {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  p {
    margin: 0;
  }

  .bad {
    color: var(--bad, #d33);
  }
</style>
