<script lang="ts">
  // Using DJINN GO from Subsonic phone apps: the address, your username,
  // and an app password just for them.
  import { onMount } from 'svelte'
  import { api } from '../lib/api'
  import { app, toast } from '../lib/state.svelte'

  let password = $state<string | null>(null)
  let shown = $state(false)
  let loaded = $state(false)

  onMount(async () => {
    try {
      password = (await api.appPassword()).password
    } finally {
      loaded = true
    }
  })

  async function make() {
    if (password && !confirm('Make a new app password? Apps using the old one stop working until you give them the new one.')) return
    try {
      password = (await api.newAppPassword()).password
      shown = true
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not make one', true)
    }
  }

  async function remove() {
    if (!confirm('Remove the app password? Apps using it stop working.')) return
    try {
      await api.removeAppPassword()
      password = null
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
        {#if password}
          <span class="mono">{shown ? password : '••••••••••••••••••••'}</span>
          <button class="small-button" onclick={() => (shown = !shown)}>{shown ? 'Hide' : 'Show'}</button>
          <button class="small-button" onclick={() => password && copy(password)}>Copy</button>
        {:else}
          <span class="muted">Make an app password first</span>
        {/if}
      </dd>
    </dl>
    <p class="small muted">
      The app password works only in these apps, never to sign in here, and skips two-step sign-in, so keep it to your own devices.
    </p>
    <div class="buttons">
      <button class="small-button" onclick={make}>{password ? 'Make a new app password' : 'Make an app password'}</button>
      {#if password}<button class="small-button" onclick={remove}>Remove it</button>{/if}
    </div>
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
