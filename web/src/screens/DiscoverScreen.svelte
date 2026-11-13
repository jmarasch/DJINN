<script lang="ts">
  // Discover: shelves to find something to play. A tap on a song queues it.
  import Discover from '../lib/Discover.svelte'
  import Heart from '../lib/Heart.svelte'
  import { t } from '../lib/i18n.svelte'
  import PageTitle from '../lib/PageTitle.svelte'
  import TrackRow from '../lib/TrackRow.svelte'
  import TrackTile from '../lib/TrackTile.svelte'
</script>

<PageTitle title={t('nav.discover')} subtitle={t('discover.subtitle')} />
<Discover>
  {#snippet songTile(track)}
    <div class="song"><TrackTile {track} /></div>
  {/snippet}
  {#snippet songRow(track)}
    <TrackRow {track} />
  {/snippet}
  {#snippet like(kind, id, name)}
    <Heart {kind} {id} {name} size={18} />
  {/snippet}
</Discover>

<style>
  .song {
    flex: none;
    width: 150px;
  }
</style>
