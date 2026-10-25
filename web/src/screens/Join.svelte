<script lang="ts">
  // Shown on the host's screen (or a TV) so guests can scan in.
  import { onMount } from 'svelte'

  let url = $state('')

  onMount(async () => {
    const res = await fetch('/api/join')
    if (res.ok) url = (await res.json()).url
  })
</script>

<div class="join">
  <h1>Join the party</h1>
  <div class="qr">
    <img src="/api/join/qr.svg" alt="QR code for {url}" />
  </div>
  {#if url}<p class="url">{url}</p>{/if}

  <ol>
    <li>Scan the code with your phone camera, or type the address above.</li>
    <li>Sign in. No account yet? Ask the host for one.</li>
    <li>Search for a song or browse the library, then tap <b>+</b> to add it to the party queue.</li>
  </ol>
  <p class="muted">Requests play in order. When the queue is empty, the party mix keeps going.</p>
</div>

<style>
  .join {
    min-height: 100dvh;
    max-width: 640px;
    margin: 0 auto;
    padding: 32px 16px;
    display: flex;
    flex-direction: column;
    align-items: center;
    text-align: center;
  }

  h1 {
    font-size: clamp(2rem, 6vw, 3rem);
    color: var(--accent);
    margin: 0 0 24px;
  }

  .qr {
    width: min(360px, 80vw);
    padding: 16px;
    border-radius: 20px;
    background: #fff;
  }

  .qr img {
    display: block;
    width: 100%;
    height: auto;
  }

  .url {
    margin: 16px 0 8px;
    font-size: 1.25rem;
    font-weight: 600;
    word-break: break-all;
  }

  ol {
    margin: 16px 0;
    padding-left: 1.4em;
    text-align: left;
    font-size: 1.125rem;
    line-height: 1.6;
  }
</style>
