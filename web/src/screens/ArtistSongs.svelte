<script lang="ts">
  // Every song by an artist, on a page of its own (from Show all on the
  // artist's page).
  import { api } from '../lib/api'
  import Icon from '../lib/Icon.svelte'
  import InfiniteList from '../lib/InfiniteList.svelte'
  import QueueAll from '../lib/QueueAll.svelte'
  import TrackRow from '../lib/TrackRow.svelte'

  let { id }: { id: number } = $props()
  let name = $state('')
  let total = $state<number | null>(null)

  $effect(() => {
    name = ''
    api
      .artist(id)
      .then((a) => (name = a.name))
      .catch(() => {})
  })

  async function load(offset: number) {
    const page = await api.artistSongs(id, offset, 100)
    total = page.total
    return { items: page.tracks, total: page.total }
  }
</script>

<button class="back" onclick={() => history.back()}><Icon name="back" /> Back</button>

<h1>{name ? `Songs by ${name}` : 'Songs'}</h1>
<div class="head">
  {#if total !== null}<p class="small muted">{total.toLocaleString()} {total === 1 ? 'song' : 'songs'}</p>{/if}
  <QueueAll kind="artist" refId={id} />
</div>
<InfiniteList cacheKey="artist-songs-{id}" {load} key={(t) => t.id}>
  {#snippet item(track)}
    <TrackRow {track} />
  {/snippet}
</InfiniteList>

<style>
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    margin-bottom: 8px;
  }
</style>
