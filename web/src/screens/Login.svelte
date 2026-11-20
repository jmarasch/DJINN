<script lang="ts">
  import { api } from '../lib/api'
  import Brand from '../lib/Brand.svelte'
  import { app, connect } from '../lib/state.svelte'
  import { t } from '../lib/i18n.svelte'
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
      error = err instanceof Error ? err.message : t('li.failed')
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
  <Brand name={title} />
  {#if ticket}
    <p class="muted">{t('li.code_help')}</p>
    <label>
      {t('signin.code')}
      <!-- svelte-ignore a11y_autofocus -->
      <input bind:value={code} inputmode="numeric" autocomplete="one-time-code" autofocus required />
    </label>
    {#if error}<p class="error">{error}</p>{/if}
    <button class="primary" disabled={busy}>{busy ? t('signin.checking') : t('signin.sign_in')}</button>
    <button type="button" class="link" onclick={() => ((ticket = null), (code = ''), (error = ''))}>{t('li.start_over')}</button>
  {:else}
    <p class="muted">{subtitle ?? (app.mode?.guest_mode ? t('li.host') : t('li.to_queue'))}</p>
    <label>
      {t('signin.username')}
      <input bind:value={username} autocomplete="username" autocapitalize="none" required />
    </label>
    <label>
      {t('signin.password')}
      <input type="password" bind:value={password} autocomplete="current-password" required />
    </label>
    {#if error}<p class="error">{error}</p>{/if}
    <button class="primary" disabled={busy}>{busy ? t('signin.signing_in') : t('signin.sign_in')}</button>
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
