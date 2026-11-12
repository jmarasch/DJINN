<script lang="ts">
  // The main page of a DJINN nobody has set up: there's no account to sign in
  // with yet, so it says so and shows the way to the first-start page.
  import { onMount } from 'svelte'
  import { api } from './api'

  type Status = Awaited<ReturnType<typeof api.bootstrapStatus>>
  let { status }: { status: Status } = $props()

  /** On the box's own screen (the kiosk opens /screen). */
  const onScreen = new URLSearchParams(location.search).has('screen')
  // svelte-ignore state_referenced_locally
  let live = $state<Status>(status)
  const setupUrl = $derived(`${live.address ?? location.origin}/setup`)

  onMount(() => {
    // Set up from a phone: move on when it's done.
    const timer = setInterval(async () => {
      try {
        const node = await fetch('/api/node/status')
        if (node.ok && node.headers.get('content-type')?.includes('json')) {
          location.replace(onScreen ? '/node?screen=1' : '/node')
          return
        }
        live = await api.bootstrapStatus()
        if (!live.needed) location.replace(onScreen ? '/tv' : '/')
      } catch {
        // Restarting; the next look finds out.
      }
    }, 3000)
    return () => clearInterval(timer)
  })
</script>

<div class="card notice">
  <h1>This DJINN hasn't been set up yet</h1>
  {#if live.open}
    <p class="muted">Scan the code with a phone, or open the link on any computer on this network, to set it up.</p>
    <img src="/api/auth/bootstrap/qr.svg" alt="QR code for {setupUrl}" />
    <p><a href={onScreen ? '/setup?screen=1' : '/setup'}>{setupUrl}</a></p>
    {#if live.mains?.length}
      <p class="muted">Found {live.mains[0].name} on this network, so this box may be meant as a node for it.</p>
    {/if}
  {:else}
    <p class="muted">Open the setup link DJINN printed when it started. It's in the server's log too.</p>
  {/if}
</div>

<style>
  .notice {
    text-align: center;
  }

  img {
    display: block;
    width: min(240px, 60vw);
    margin: 12px auto;
    border-radius: 12px;
    background: #fff;
    padding: 12px;
  }
</style>
