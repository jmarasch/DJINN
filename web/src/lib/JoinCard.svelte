<script lang="ts">
  // The join QR code with steps for guests. Used by the /join page and the
  // Join button in the app.
  import { onMount } from 'svelte'
  import PublicNetworkNote from './PublicNetworkNote.svelte'

  let { big = false }: { big?: boolean } = $props()
  let url = $state('')
  let guestMode = $state(false)

  onMount(async () => {
    const [join, mode] = await Promise.all([fetch('/api/join'), fetch('/api/auth/mode')])
    if (join.ok) url = (await join.json()).url
    if (mode.ok) guestMode = (await mode.json()).guest_mode
  })
</script>

<div class="join" class:big>
  <PublicNetworkNote />
  <div class="qr">
    <img src="/api/join/qr.svg" alt="QR code for {url}" />
  </div>
  {#if url}<p class="url">{url}</p>{/if}
  <ol>
    <li>Scan the code with your phone camera, or type the address above.</li>
    {#if guestMode}
      <li>Type your name. No account needed.</li>
    {:else}
      <li>Sign in. No account yet? Ask the host for one.</li>
    {/if}
    <li>Search for a song or browse the library, then tap it to add it to the party queue.</li>
  </ol>
  <p class="muted small">Requests play in order. When the queue is empty, the party mix keeps going.</p>
</div>

<style>
  .join {
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
  }

  .qr {
    width: min(280px, 72vw);
    padding: 14px;
    border-radius: 18px;
    background: #fff;
  }

  .big .qr {
    width: min(360px, 80vw);
  }

  .qr img {
    display: block;
    width: 100%;
    height: auto;
  }

  .url {
    margin: 14px 0 4px;
    font-size: 1.125rem;
    font-weight: 600;
    word-break: break-all;
  }

  .big .url {
    font-size: 1.25rem;
  }

  ol {
    margin: 12px 0;
    padding-left: 1.4em;
    text-align: left;
    line-height: 1.55;
  }

  .big ol {
    font-size: 1.125rem;
  }
</style>
