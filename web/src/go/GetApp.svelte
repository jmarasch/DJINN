<script lang="ts">
  // Offers the Android app, from this server, on Android.
  import { t } from '../lib/i18n.svelte'
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
    <h2>{t('ga.title')}</h2>
    <p class="small muted">
      {t('ga.note')}
    </p>
    <a class="button" href="/go/app/djinn-go.apk" download="djinn-go.apk">
      {t('ga.download', { n: Math.round(release.size / 1_000_000) })}
    </a>
    <p class="small muted sum">{t('gp.app_version', { version: release.version, sum: release.sha256 })}</p>
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
