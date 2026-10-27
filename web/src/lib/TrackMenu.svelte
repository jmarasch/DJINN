<script lang="ts">
  // A song's extra actions, behind a "more" button.
  import AddToPlaylist from './AddToPlaylist.svelte'
  import { api } from './api'
  import GenreChips from './GenreChips.svelte'
  import Icon from './Icon.svelte'
  import Sheet from './Sheet.svelte'
  import { isAdmin, toast } from './state.svelte'
  import type { Track, TrackTags } from './types'

  let { track }: { track: Track } = $props()
  let menu = $state(false)
  let playlist = $state(false)
  let tags = $state<TrackTags | null>(null)

  // The song's genres and styles, once the menu opens.
  $effect(() => {
    if (!menu || tags) return
    api
      .trackTags(track.id)
      .then((t) => (tags = t))
      .catch(() => {})
  })

  function similar() {
    menu = false
    location.hash = `/similar/${track.id}`
  }

  async function radio() {
    menu = false
    try {
      await api.playRadio({ kind: 'track', id: track.id })
      toast(`Radio from ${track.title}`)
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not start the radio', true)
    }
  }
</script>

<button class="icon-only" aria-label="More for {track.title}" onclick={() => (menu = true)}>
  <Icon name="more" size={20} />
</button>
{#if menu}
  <Sheet title={track.title} onclose={() => (menu = false)}>
    {#if tags?.genres.length || tags?.styles.length}
      <div class="tags">
        {#if tags.genres.length}
          <span class="small muted">Genres</span>
          <GenreChips genres={tags.genres} onopen={() => (menu = false)} />
        {/if}
        {#if tags.styles.length}
          <span class="small muted">Sounds like</span>
          <GenreChips genres={tags.styles} onopen={() => (menu = false)} />
        {/if}
      </div>
    {/if}
    <button class="item" onclick={similar}><Icon name="similar" /> More like this</button>
    {#if isAdmin()}
      <button class="item" onclick={radio}><Icon name="radio" /> Start radio from this song</button>
      <button
        class="item"
        onclick={() => {
          menu = false
          playlist = true
        }}><Icon name="playlist" /> Add to playlist</button
      >
    {/if}
  </Sheet>
{/if}
<AddToPlaylist kind="track" refId={track.id} label={track.title} trigger={false} bind:open={playlist} />

<style>
  .icon-only {
    flex: none;
    width: 40px;
    min-height: 40px;
    padding: 0;
    background: transparent;
    color: var(--muted);
  }

  .tags {
    display: grid;
    grid-template-columns: max-content 1fr;
    align-items: center;
    gap: 8px 12px;
    padding: 4px 4px 12px;
    border-bottom: 1px solid var(--line);
  }

  .item {
    width: 100%;
    justify-content: flex-start;
    gap: 12px;
    min-height: 52px;
    padding: 0 4px;
    border-radius: 0;
    border-bottom: 1px solid var(--line);
    background: transparent;
  }
</style>
