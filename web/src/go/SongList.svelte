<script lang="ts">
  // Songs that play as a list: tapping one plays it and carries on with
  // the ones after it.
  import type { Track } from '../lib/types'
  import { playList, tapSong } from './player.svelte'
  import SongRow from './SongRow.svelte'

  let { tracks, numbered = false }: { tracks: Track[]; numbered?: boolean } = $props()
</script>

{#each tracks as track, i (`${track.id}-${i}`)}
  <SongRow {track} number={numbered ? (track.track_no ?? i + 1) : null} art={!numbered} onplay={() => tapSong(track, () => playList(tracks, i))} />
{/each}
