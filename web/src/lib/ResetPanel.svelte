<script lang="ts">
  // Admin > System: clearing things out. Each one asks first, and each
  // says to back up before going ahead, since none can be undone.
  import { t } from './i18n.svelte'
  import { api } from './api'
  import { downloadUnlocked, unlocked } from './protected.svelte'
  import { reloadAfterRestart } from './restart'
  import { toast } from './state.svelte'
  import type { SystemReset } from './types'

  /** Each reset's wording: `rs.<key>`, `rs.<key>_about` and `rs.<key>_q`. */
  const entry = (what: SystemReset, key: string) => ({
    what,
    get label() {
      return t(`rs.${key}`)
    },
    get about() {
      return t(`rs.${key}_about`)
    },
    get confirm() {
      return t(`rs.${key}_q`)
    },
  })
  const resets: { what: SystemReset; label: string; about: string; confirm: string }[] = [
    entry('queue', 'queue'),
    entry('tag_edits', 'tags'),
    entry('imported_lyrics', 'lyrics'),
    entry('analysis', 'analysis'),
    entry('listening', 'listening'),
    entry('history', 'history'),
    entry('settings', 'settings'),
    entry('everything', 'everything'),
  ]

  let busy = $state<SystemReset | null>(null)
  let restarting = $state(false)
  /** Reset completely: there's no account to come back to on this device. */
  let wiped = $state(false)

  $effect(() => {
    if (restarting && !wiped) return reloadAfterRestart()
  })

  async function run(item: (typeof resets)[number]) {
    const warning = `${item.confirm}\n\n${t('rs.cant_undo')}`
    if (item.what === 'everything') {
      const typed = prompt(`${warning}\n\n${t('rs.type_reset')}`)
      if (typed?.trim().toUpperCase() !== 'RESET') return
    } else if (!confirm(warning)) {
      return
    }
    busy = item.what
    try {
      const done = await unlocked(() => api.systemReset(item.what))
      wiped = item.what === 'everything'
      if (done.restarting) restarting = true
      else toast(t('common.done'))
    } catch (e) {
      toast(e instanceof Error ? e.message : t('common.failed'), true)
    } finally {
      busy = null
    }
  }
</script>

<section class="card">
  <h2>{t('rs.title')}</h2>
  {#if wiped}
    <p class="small">
      {t('rs.wiped')}
    </p>
  {:else if restarting}
    <p class="small"><span class="spinner" aria-hidden="true"></span> {t('rs.restarting')}</p>
  {:else}
    {#each resets as reset (reset.what)}
      <div class="reset">
        <p class="small">{reset.about}</p>
        <button class="danger" disabled={busy !== null} onclick={() => run(reset)}>{reset.label}</button>
        <p class="small muted note">
          {t('rs.note')} <a href="/api/backup" download onclick={(e) => { e.preventDefault(); downloadUnlocked('/api/backup') }}>{t('rs.download')}</a>.
        </p>
      </div>
    {/each}
  {/if}
</section>

<style>
  .reset {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 8px;
    padding: 12px 0;
    border-top: 1px solid var(--line);
  }

  .reset:first-of-type {
    border-top: 0;
    padding-top: 0;
  }

  p {
    margin: 0;
  }
</style>
