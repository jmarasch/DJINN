<script lang="ts">
  import { t } from './i18n.svelte'
  import { api } from './api'
  import { app, toast } from './state.svelte'
  import type { IdentificationRow } from './types'

  let found = $state<IdentificationRow[]>([])

  async function load() {
    const review = await api.identify()
    found = review.found
    app.identify = review
  }

  $effect(() => {
    load().catch((e) => toast(e.message, true))
  })

  // New finds come in while it runs; reload the list as they do.
  $effect(() => {
    void app.identify?.found
    api
      .identify()
      .then((r) => (found = r.found))
      .catch(() => {})
  })

  async function attempt(action: () => Promise<unknown>, done?: string) {
    try {
      await action()
      if (done) toast(done)
      await load()
    } catch (e) {
      toast(e instanceof Error ? e.message : t('common.failed'), true)
    }
  }

  const status = $derived(app.identify)
  const fillIn = (row: IdentificationRow) =>
    [
      !row.current_album && row.album ? t('ident.album', { v: row.album }) : null,
      !row.current_year && row.year ? t('ident.year', { v: row.year }) : null,
      !row.current_artist && row.artist ? t('ident.artist', { v: row.artist }) : null,
      row.genres ? t('ident.genres', { v: row.genres }) : null,
    ].filter(Boolean)
</script>

<section class="card">
  <h2>{t('ident.title')}</h2>
  <p class="small muted">
    {t('ident.intro')}
  </p>
  {#if status}
    {#if status.needs_key || status.needs_contact}
      <p class="small muted">
        {status.needs_key && status.needs_contact
          ? t('ident.need_both')
          : status.needs_key
            ? t('ident.need_key')
            : t('ident.need_contact')}
      </p>
    {:else}
      <p class="small">
        {#if status.running}<span class="spinner" aria-hidden="true"></span> {t('ident.listening')}{/if}
        {t('ident.waiting', { n: status.waiting, applied: status.applied })}
      </p>
      {#if status.running}
        <button onclick={() => attempt(() => api.stopIdentify(), t('ident.stopping'))}>{t('common.stop')}</button>
      {:else}
        <button disabled={!status.waiting} onclick={() => attempt(() => api.startIdentify())}>{t('ident.start')}</button>
      {/if}
    {/if}
  {/if}

  {#if found.length}
    <div class="review-head">
      <h3>{t('ident.found', { n: found.length })}</h3>
      <button
        class="small-button primary"
        onclick={() => attempt(() => api.applyIdentified(found.map((f) => f.track_id)), t('ident.filled'))}>{t('ident.use_all')}</button
      >
    </div>
    {#each found as row (row.track_id)}
      <div class="row">
        <div class="grow">
          <div class="ellipsis">{row.current_title} <span class="muted small">{row.current_artist ?? ''}</span></div>
          <div class="small accent">{fillIn(row).join(' · ') || t('ident.nothing_new')}</div>
          {#if row.score}<div class="small muted">{t('ident.match', { pct: Math.round(row.score * 100), title: row.title ?? '', artist: row.artist ?? '' })}</div>{/if}
        </div>
        <div class="actions">
          <button class="small-button" onclick={() => attempt(() => api.applyIdentified([row.track_id]))}>{t('ident.use')}</button>
          <button class="small-button" onclick={() => attempt(() => api.dismissIdentified([row.track_id]))}>{t('common.skip')}</button>
        </div>
      </div>
    {/each}
  {/if}
</section>

<style>
  .review-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  h3 {
    margin: 0;
    font-size: 1rem;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 0;
    border-bottom: 1px solid var(--line);
  }

  .grow {
    flex: 1;
    min-width: 0;
  }

  .accent {
    color: var(--accent);
  }

  .actions {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .small-button {
    min-height: 32px;
  }
</style>
