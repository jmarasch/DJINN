<script lang="ts">
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
  <h1>DJINN</h1>
  {#if !changing && welcomeBack}
    <p class="welcome">Welcome back, <b>{welcomeBack}</b>!</p>
    {#if error}<p class="error">{error}</p>{/if}
    <button class="primary" disabled={busy} onclick={() => join()}>
      {busy ? 'Joining...' : `Continue as ${welcomeBack}`}
    </button>
    <button class="link" onclick={() => ((changing = true), (name = ''))}>Not you? Pick a name</button>
  {:else}
    <form onsubmit={join}>
      <label>
        What should we call you?
        <!-- svelte-ignore a11y_autofocus -->
        <input bind:value={name} maxlength="32" autocomplete="nickname" placeholder="Your name" autofocus required />
      </label>
      <p class="muted small">Shown next to the songs you add. No account needed.</p>
      {#if error}<p class="error">{error}</p>{/if}
      <button class="primary" disabled={busy || !name.trim()}>{busy ? 'Joining...' : 'Join the party'}</button>
    </form>
  {/if}
</div>

<button class="link host" onclick={() => (app.hostLogin = true)}>Host sign in</button>

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

  .host {
    display: block;
    margin: 8px auto 0;
  }
</style>
