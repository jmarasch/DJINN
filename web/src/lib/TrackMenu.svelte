<script lang="ts">
  // A song's extra actions, behind a "more" button.
  import AddToPlaylist from './AddToPlaylist.svelte'
  import Icon from './Icon.svelte'
  import Sheet from './Sheet.svelte'
  import { isAdmin } from './state.svelte'
  import type { Track } from './types'

  let { track }: { track: Track } = $props()
  let menu = $state(false)
  let playlist = $state(false)

  function similar() {
    menu = false
    location.hash = `/similar/${track.id}`
  }
</script>

<button class="icon-only" aria-label="More for {track.title}" onclick={() => (menu = true)}>
  <Icon name="more" size={20} />
</button>
{#if menu}
  <Sheet title={track.title} onclose={() => (menu = false)}>
    <button class="item" onclick={similar}><Icon name="similar" /> More like this</button>
    {#if isAdmin()}
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
