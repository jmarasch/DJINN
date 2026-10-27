<script lang="ts">
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
      toast(e instanceof Error ? e.message : 'That did not work', true)
    }
  }

  const status = $derived(app.identify)
  const fillIn = (row: IdentificationRow) =>
    [
      !row.current_album && row.album ? `album ${row.album}` : null,
      !row.current_year && row.year ? `year ${row.year}` : null,
      !row.current_artist && row.artist ? `artist ${row.artist}` : null,
      row.genres ? `genres ${row.genres}` : null,
    ].filter(Boolean)
</script>

<section class="card">
  <h2>Identify songs</h2>
  <p class="small muted">
    Finds the album, year, and genres for songs whose tags are missing them, by their sound (AcoustID and MusicBrainz).
    You choose what to use; DJINN keeps it in its own database and your files aren't changed.
  </p>
  {#if status}
    {#if status.needs_key || status.needs_contact}
      <p class="small muted">
        Add {status.needs_key ? 'an AcoustID key' : ''}{status.needs_key && status.needs_contact ? ' and ' : ''}{status.needs_contact
          ? 'a MusicBrainz contact'
          : ''} under <a href="#/admin/rules">Rules</a> first.
      </p>
    {:else}
      <p class="small">
        {#if status.running}<span class="spinner" aria-hidden="true"></span> Listening for matches...{/if}
        {status.waiting} {status.waiting === 1 ? 'song' : 'songs'} to check, {status.applied} filled in so far.
      </p>
      {#if status.running}
        <button onclick={() => attempt(() => api.stopIdentify(), 'Stopping after this song')}>Stop</button>
      {:else}
        <button disabled={!status.waiting} onclick={() => attempt(() => api.startIdentify())}>Identify songs</button>
      {/if}
    {/if}
  {/if}

  {#if found.length}
    <div class="review-head">
      <h3>Found {found.length}</h3>
      <button
        class="small-button primary"
        onclick={() => attempt(() => api.applyIdentified(found.map((f) => f.track_id)), 'Filled in')}>Use all</button
      >
    </div>
    {#each found as row (row.track_id)}
      <div class="row">
        <div class="grow">
          <div class="ellipsis">{row.current_title} <span class="muted small">{row.current_artist ?? ''}</span></div>
          <div class="small accent">{fillIn(row).join(' · ') || 'Nothing new'}</div>
          {#if row.score}<div class="small muted">{Math.round(row.score * 100)}% match: {row.title} by {row.artist}</div>{/if}
        </div>
        <div class="actions">
          <button class="small-button" onclick={() => attempt(() => api.applyIdentified([row.track_id]))}>Use</button>
          <button class="small-button" onclick={() => attempt(() => api.dismissIdentified([row.track_id]))}>Skip</button>
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
    padding: 0 12px;
    font-size: 0.8125rem;
  }
</style>
