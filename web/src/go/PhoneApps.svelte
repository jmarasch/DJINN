<script lang="ts">
  // Using DJINN GO from Subsonic phone apps: the address, your username,
  // and an app password just for them.
  import { onMount } from 'svelte'
  import { api } from '../lib/api'
  import { app, toast } from '../lib/state.svelte'

  let exists = $state(false)
  /** The new app password, shown only until this page closes. */
  let made = $state<string | null>(null)
  let shown = $state(false)
  let loaded = $state(false)
  /** Asking for the account's password before making one. */
  let asking = $state(false)
  let current = $state('')

  onMount(async () => {
    try {
      exists = (await api.appPassword()).exists
    } finally {
      loaded = true
    }
  })

  async function make(e: SubmitEvent) {
    e.preventDefault()
    try {
      made = (await api.newAppPassword(current)).password
      exists = true
      shown = true
      asking = false
      current = ''
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not make one', true)
    }
  }

  async function remove() {
    if (!confirm('Remove the app password? Apps using it stop working.')) return
    try {
      await api.removeAppPassword()
      exists = false
      made = null
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not remove it', true)
    }
  }

  async function copy(text: string) {
    try {
      await navigator.clipboard.writeText(text)
      toast('Copied')
    } catch {
      toast('Select it and copy it', true)
    }
  }
</script>

<section class="card">
  <h2>Phone apps</h2>
  <p class="small muted">
    Apps that speak Subsonic can play DJINN GO with downloads for offline listening (if your role allows), CarPlay and Android Auto:
    Symfonium, DSub, Ultrasonic and Tempo on Android; play:Sub, Amperfy and Substreamer on iPhone; Feishin and Sonixd on computers.
    Add a Subsonic (or OpenSubsonic) server with:
  </p>
  {#if loaded}
    <dl>
      <dt>Server</dt>
      <dd><span class="mono">{location.origin}</span> <button class="small-button" onclick={() => copy(location.origin)}>Copy</button></dd>
      <dt>Username</dt>
      <dd class="mono">{app.me?.username}</dd>
      <dt>Password</dt>
      <dd>
        {#if made}
          <span class="mono">{shown ? made : '••••••••••••••••••••'}</span>
          <button class="small-button" onclick={() => (shown = !shown)}>{shown ? 'Hide' : 'Show'}</button>
          <button class="small-button" onclick={() => made && copy(made)}>Copy</button>
        {:else if exists}
          <span class="muted">You have one. It's only shown when it's made; make a new one if you've lost it.</span>
        {:else}
          <span class="muted">Make an app password first</span>
        {/if}
      </dd>
    </dl>
    <p class="small muted">
      The app password works only in these apps, never to sign in here, and skips two-step sign-in, so keep it to your own devices.
      Changing your account's password removes it, so apps will need a new one after that.
    </p>
    {#if made}<p class="small muted">Copy it now: it isn't shown again.</p>{/if}
    {#if asking}
      <form class="buttons" onsubmit={make}>
        <input type="password" bind:value={current} autocomplete="current-password" placeholder="Your password" required aria-label="Your password" />
        <button class="small-button">Make it</button>
        {#if exists}<span class="small muted">Apps using the old one stop working until you give them the new one.</span>{/if}
      </form>
    {:else}
      <div class="buttons">
        <button class="small-button" onclick={() => (asking = true)}>{exists ? 'Make a new app password' : 'Make an app password'}</button>
        {#if exists}<button class="small-button" onclick={remove}>Remove it</button>{/if}
      </div>
    {/if}
  {/if}
</section>

<style>
  dl {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 8px 12px;
    align-items: center;
    margin: 0;
  }

  dt {
    color: var(--muted);
  }

  dd {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    margin: 0;
    min-width: 0;
  }

  .mono {
    font-family: ui-monospace, monospace;
    overflow-wrap: anywhere;
  }

  p {
    margin: 0;
  }

  .buttons {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
</style>
