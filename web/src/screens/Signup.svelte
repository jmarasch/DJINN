<script lang="ts">
  // Creating an account: from an invite link (/signup?invite=...), or from
  // the sign-in screen while sign-ups are open or reviewed.
  import { onMount } from 'svelte'
  import { api } from '../lib/api'
  import { app, connect } from '../lib/state.svelte'
  import { t } from '../lib/i18n.svelte'
  import type { User } from '../lib/types'

  let { onback, ondone }: { onback?: () => void; ondone?: (me: User) => void } = $props()

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
      inviteError = e instanceof Error ? e.message : t('su.bad_invite')
    }
  })

  function toSignIn() {
    if (invite) history.replaceState(null, '', '/')
    onback?.()
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
      const result = await api.signUp(username, password, invite || undefined)
      if ('pending' in result) {
        waiting = true
      } else {
        if (invite) history.replaceState(null, '', '/')
        app.me = result
        if (ondone) ondone(result)
        else connect()
      }
    } catch (err) {
      error = err instanceof Error ? err.message : t('su.failed')
    } finally {
      busy = false
    }
  }
</script>

{#if waiting}
  <div class="card">
    <h1>{t('su.almost')}</h1>
    <p class="muted">{t('su.waiting')}</p>
    <button class="primary" onclick={toSignIn}>{t('setup.to_sign_in')}</button>
  </div>
{:else if inviteError}
  <div class="card">
    <h1>{t('su.invite')}</h1>
    <p class="error">{inviteError}</p>
    <p class="muted">{t('su.ask_new')}</p>
    <button onclick={toSignIn}>{t('setup.to_sign_in')}</button>
  </div>
{:else}
  <form class="card" onsubmit={submit}>
    <h1>{t('su.create')}</h1>
    {#if invite}
      <p class="muted">
        {inviteRole ? t('su.invited', { role: inviteRole }) : t('su.checking')}
      </p>
    {:else if app.mode?.signup === 'review'}
      <p class="muted">{t('su.review')}</p>
    {/if}
    <label>
      {t('signin.username')}
      <input bind:value={username} autocomplete="username" autocapitalize="none" maxlength="32" required />
    </label>
    <label>
      {t('signin.password')}
      <input type="password" bind:value={password} autocomplete="new-password" minlength="8" required />
    </label>
    <label>
      {t('setup.confirm')}
      <input type="password" bind:value={confirm} autocomplete="new-password" required />
    </label>
    <p class="small muted">{t('su.password_hint')}</p>
    {#if error}<p class="error">{error}</p>{/if}
    <button class="primary" disabled={busy || (!!invite && !inviteRole)}>
      {busy ? t('setup.creating') : t('su.create_button')}
    </button>
    <button type="button" class="link" onclick={toSignIn}>{t('su.have_one')}</button>
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
