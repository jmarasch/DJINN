<script lang="ts">
  import { t } from '../lib/i18n.svelte'
  import Brand from '../lib/Brand.svelte'
  import LanguagePicker from '../lib/LanguagePicker.svelte'
  import { api } from '../lib/api'
  import { app, connect } from '../lib/state.svelte'

  const welcomeBack = app.mode?.welcome_back ?? null
  let name = $state(welcomeBack ?? '')
  let changing = $state(welcomeBack === null)
  let error = $state('')
  let busy = $state(false)

  async function join(e?: SubmitEvent) {
    e?.preventDefault()
    busy = true
    error = ''
    try {
      app.me = await api.joinAsGuest(name)
      connect()
    } catch (err) {
      error = err instanceof Error ? err.message : 'Could not join'
    } finally {
      busy = false
    }
  }
</script>

<div class="card">
  <Brand />
  {#if !changing && welcomeBack}
    <p class="welcome">{t('join.welcome_back', { name: welcomeBack })}</p>
    {#if error}<p class="error">{error}</p>{/if}
    <button class="primary" disabled={busy} onclick={() => join()}>
      {busy ? t('join.joining') : t('join.continue_as', { name: welcomeBack })}
    </button>
    <button class="link" onclick={() => ((changing = true), (name = ''))}>{t('join.not_you')}</button>
  {:else}
    <form onsubmit={join}>
      <label>
        {t('join.your_name_question')}
        <!-- svelte-ignore a11y_autofocus -->
        <input bind:value={name} maxlength="32" autocomplete="nickname" placeholder={t('join.your_name')} autofocus required />
      </label>
      <p class="muted small">{t('join.name_note')}</p>
      {#if error}<p class="error">{error}</p>{/if}
      <button class="primary" disabled={busy || !name.trim()}>{busy ? t('join.joining') : t('join.join')}</button>
    </form>
  {/if}
</div>

<div class="language"><LanguagePicker /></div>

<button class="link host" onclick={() => (app.hostLogin = true)}>{t('join.host_sign_in')}</button>

<style>
  form {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .welcome {
    margin: 0;
    font-size: 1.125rem;
  }

  .link {
    min-height: 40px;
    background: transparent;
    color: var(--muted);
    text-decoration: underline;
  }

  .language {
    display: flex;
    justify-content: center;
    margin-top: 12px;
  }

  .host {
    display: block;
    margin: 8px auto 0;
  }
</style>
