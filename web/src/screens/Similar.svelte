<script lang="ts">
  // Songs that sound like one song.
  import { t } from '../lib/i18n.svelte'
  import { api } from '../lib/api'
  import Art from '../lib/Art.svelte'
  import Credits from '../lib/Credits.svelte'
  import Icon from '../lib/Icon.svelte'
  import TrackRow from '../lib/TrackRow.svelte'
  import { isAdmin } from '../lib/state.svelte'
  import type { Track } from '../lib/types'

  let { id }: { id: number } = $props()
  let seed = $state<Track | null>(null)
  let similar = $state<Track[] | null>(null)
  let analyzed = $state(true)
  let analysisOn = $state(true)
  let error = $state('')

  $effect(() => {
    seed = null
    similar = null
    Promise.all([api.track(id), api.similarTracks(id)])
      .then(([track, result]) => {
        seed = track
        similar = result.tracks
        analyzed = result.analyzed
        analysisOn = result.analysis_on
      })
      .catch((e) => (error = e.message))
  })
</script>

<button class="back" onclick={() => history.back()}><Icon name="back" /> {t('nav.back')}</button>

{#if error}
  <p class="error">{error}</p>
{:else if seed && similar}
  <header>
    <Art hash={seed.art_hash} size={72} round={10} />
    <div class="grow">
      <div class="muted small">{t('sim.more_like')}</div>
      <h1 class="ellipsis">{seed.title}</h1>
      <div class="muted ellipsis"><Credits track={seed} /></div>
    </div>
  </header>
  {#if !analyzed && !analysisOn}
    <p class="muted">
      {t('sim.off')}
      {#if isAdmin()}{t('sim.switch_on')} <a href="#/admin/library">{t('sim.admin_library')}</a>.{/if}
    </p>
  {:else if !analyzed}
    <p class="muted">
      {t('sim.not_yet')}
    </p>
  {:else}
    <p class="small muted">{t('sim.note')}</p>
    {#each similar as track (track.id)}
      <TrackRow {track} />
    {/each}
  {/if}
{:else}
  <p class="muted">{t('sim.listening')}</p>
{/if}

<style>
  header {
    display: flex;
    align-items: center;
    gap: 14px;
    margin-bottom: 8px;
  }

  .grow {
    flex: 1;
    min-width: 0;
  }

  h1 {
    margin: 2px 0;
    font-size: 1.375rem;
  }
</style>
