<script lang="ts">
  import { onMount } from 'svelte'

  type Health = { status: string; version: string }

  let health = $state<Health | null>(null)
  let failed = $state(false)

  onMount(async () => {
    try {
      const res = await fetch('/api/health')
      if (!res.ok) throw new Error(`HTTP ${res.status}`)
      health = await res.json()
    } catch {
      failed = true
    }
  })
</script>

<main>
  <h1>DJINN</h1>
  <p class="tagline">Dynamic Jukebox with Intelligent Neural Navigation</p>

  <div class="status">
    {#if health}
      <span class="dot ok"></span> Server online, v{health.version}
    {:else if failed}
      <span class="dot bad"></span> Server unreachable
    {:else}
      <span class="dot"></span> Connecting...
    {/if}
  </div>
</main>

<style>
  main {
    min-height: 100dvh;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 24px 16px;
    text-align: center;
  }

  h1 {
    margin: 0;
    font-size: clamp(2.5rem, 12vw, 4rem);
    letter-spacing: 0.2em;
    color: var(--accent);
  }

  .tagline {
    margin: 8px 0 32px;
    color: var(--muted);
  }

  .status {
    display: inline-flex;
    align-items: center;
    gap: 10px;
    padding: 12px 20px;
    border-radius: 999px;
    background: var(--surface);
  }

  .dot {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background: var(--muted);
  }

  .dot.ok {
    background: var(--ok);
  }

  .dot.bad {
    background: var(--bad);
  }
</style>
