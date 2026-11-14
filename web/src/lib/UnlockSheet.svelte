<script lang="ts">
  // Asks for the admin's password (and a two-step code at the strictest
  // level) to open the protected areas.
  import { onMount } from 'svelte'
  import { api } from './api'
  import Sheet from './Sheet.svelte'
  import type { ProtectedStatus } from './types'

  let { onopened, onclose }: { onopened: () => void; onclose: () => void } = $props()

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
        if (s.open) onopened()
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

<Sheet title="Unlock protected areas" {onclose}>
  <form onsubmit={submit}>
    <p class="small muted">
      This changes your music library. Enter your password{status?.level === 'max' ? ' and a code from your authenticator app' : ''}
      to unlock it. It locks again when you leave Admin, or after ten minutes without use.
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
</Sheet>

<style>
  form {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .bad {
    color: var(--danger, #d33);
  }
</style>
