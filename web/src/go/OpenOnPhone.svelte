<script lang="ts">
  // A QR code for opening DJINN GO on a phone.
  import { onMount } from 'svelte'
  import { api } from '../lib/api'

  let address = $state<{ url: string; qr_svg: string; others: string[] } | null>(null)

  onMount(async () => {
    try {
      address = await api.goAddress(location.origin)
    } catch {
      address = null
    }
  })
</script>

{#if address}
  <section class="card">
    <h2>Open on your phone</h2>
    <p class="small muted">Scan this with your phone's camera. Then add it to your home screen to use GO like an app.</p>
    <!-- The QR code is an SVG DJINN made itself. -->
    <div class="qr">{@html address.qr_svg}</div>
    <p class="url mono">{address.url}</p>
    {#if address.others.length}
      <p class="small muted">Also at: {address.others.join(', ')}</p>
    {/if}
  </section>
{/if}

<style>
  .qr {
    align-self: center;
    width: 220px;
    padding: 10px;
    border-radius: 12px;
    background: #fff;
  }

  .qr :global(svg) {
    display: block;
    width: 100%;
    height: auto;
  }

  .url {
    align-self: center;
    margin: 0;
    overflow-wrap: anywhere;
    text-align: center;
  }

  .mono {
    font-family: ui-monospace, monospace;
  }

  p {
    margin: 0;
  }
</style>
