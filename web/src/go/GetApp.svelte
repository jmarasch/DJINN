<script lang="ts">
  // Offers the Android app, from this server, on Android.
  import { onMount } from 'svelte'
  import { api } from '../lib/api'
  import type { GoAppRelease } from '../lib/types'

  const android = /android/i.test(navigator.userAgent)
  let release = $state<GoAppRelease | null>(null)

  onMount(async () => {
    if (!android) return
    try {
      release = await api.goApp()
    } catch {
      release = null
    }
  })
</script>

{#if release}
  <section class="card">
    <h2>Get the Android app</h2>
    <p class="small muted">
      Downloads, offline listening, lock screen controls and more. It comes straight from this server, so Android will
      ask you to allow installing it, and may warn about where it's from.
    </p>
    <a class="button" href="/go/app/djinn-go.apk" download="djinn-go.apk">
      Download ({(release.size / 1_000_000).toFixed(0)} MB)
    </a>
    <p class="small muted sum">Version {release.version}. SHA-256: {release.sha256}</p>
  </section>
{/if}

<style>
  .button {
    display: inline-block;
    margin: 8px 0;
    padding: 10px 16px;
    border-radius: 999px;
    background: var(--accent);
    color: var(--on-accent, #fff);
    text-decoration: none;
  }

  .sum {
    overflow-wrap: anywhere;
  }
</style>
