<script lang="ts">
  import PageTitle from '../../lib/PageTitle.svelte'
  // Every song by an artist, on a page of its own (from Show all on the
  // artist's page).
  import { api } from '../../lib/api'
  import Icon from '../../lib/Icon.svelte'
  import InfiniteList from '../../lib/InfiniteList.svelte'
  import { toast } from '../../lib/state.svelte'
  import type { Track } from '../../lib/types'
  import { playList, tapSong } from '../player.svelte'
  import SongRow from '../SongRow.svelte'

  let { id }: { id: number } = $props()
  let name = $state('')
  let total = $state<number | null>(null)
  /** The songs loaded so far, for playing on from one of them. */
  let loaded: Track[] = []

  $effect(() => {
    name = ''
    loaded = []
    api
      .artist(id)
      .then((a) => (name = a.name))
      .catch(() => {})
  })

  async function load(offset: number) {
    const page = await api.artistSongs(id, offset, 100)
    total = page.total
    const known = new Set(loaded.map((t) => t.id))
    loaded = [...loaded, ...page.tracks.filter((t) => !known.has(t.id))]
    return { items: page.tracks, total: page.total }
  }

  function playFrom(track: Track) {
    const at = loaded.findIndex((t) => t.id === track.id)
    tapSong(track, () => playList(loaded, Math.max(0, at)))
  }

  /** Plays every song by the artist, loaded here or not. */
  async function playAll() {
    try {
      const page = await api.artistSongs(id, 0, 5000)
      if (page.tracks.length) playList(page.tracks)
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not play them', true)
    }
  }
</script>

<PageTitle title={name ? `Songs by ${name}` : 'Songs'} />
{#if total !== null}<p class="small muted">{total.toLocaleString()} {total === 1 ? 'song' : 'songs'}</p>{/if}
<div class="buttons">
  <button class="primary" onclick={playAll} disabled={!total}><Icon name="play" /> Play all</button>
</div>
<InfiniteList cacheKey="go-artist-songs-{id}" {load} key={(t) => t.id}>
  {#snippet item(track)}
    <SongRow {track} art onplay={() => playFrom(track)} />
  {/snippet}
</InfiniteList>

<style>

  .buttons {
    display: flex;
    gap: 8px;
    margin: 8px 0 12px;
  }
</style>
