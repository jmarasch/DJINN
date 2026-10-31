<script lang="ts">
  // A song's extra actions, behind a "more" button.
  import AddToPlaylist from './AddToPlaylist.svelte'
  import { api } from './api'
  import GenreChips from './GenreChips.svelte'
  import Icon from './Icon.svelte'
  import Sheet from './Sheet.svelte'
  import TagEditor from './TagEditor.svelte'
  import { isAdmin, toast } from './state.svelte'
  import type { TagField, Track, TrackEdit, TrackTags } from './types'

  let { track }: { track: Track } = $props()
  let menu = $state(false)
  let playlist = $state(false)
  let tags = $state<TrackTags | null>(null)
  let editing = $state<TrackEdit | null>(null)
  // The "Sounds like" genre just taken off, for Undo while the menu is open.
  let removed = $state<string | null>(null)

  async function editTags() {
    menu = false
    try {
      editing = await api.trackEdit(track.id)
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not open the tags', true)
    }
  }

  const songFields: TagField[] = ['title', 'artist', 'album', 'album_artist', 'year', 'genre']

  // The song's genres and styles, once the menu opens.
  $effect(() => {
    if (!menu || tags) return
    api
      .trackTags(track.id)
      .then((t) => (tags = t))
      .catch(() => {})
  })

  $effect(() => {
    if (!menu) removed = null
  })

  async function removeStyle(genre: string) {
    try {
      await api.removeStyle(track.id, genre)
      if (tags) tags.styles = tags.styles.filter((s) => s !== genre)
      removed = genre
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not remove it', true)
    }
  }

  async function undoRemove() {
    const genre = removed
    if (!genre) return
    try {
      await api.restoreStyle(track.id, genre)
      if (tags) tags.styles = [...tags.styles, genre]
      removed = null
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not put it back', true)
    }
  }

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
    {#if tags && (tags.genres.length || tags.styles.length || removed)}
      <div class="tags">
        {#if tags.genres.length}
          <span class="small muted">Genres</span>
          <GenreChips genres={tags.genres} onopen={() => (menu = false)} />
        {/if}
        {#if tags.styles.length || removed}
          <span class="small muted">Sounds like</span>
          <div class="styles">
            <GenreChips
              genres={tags.styles}
              onopen={() => (menu = false)}
              onremove={isAdmin() ? removeStyle : undefined}
            />
            {#if removed}
              <p class="small muted">
                Took off {removed}. <button class="link" onclick={undoRemove}>Undo</button>
              </p>
            {/if}
          </div>
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
      <button class="item" onclick={editTags}><Icon name="edit" /> Edit tags</button>
    {/if}
  </Sheet>
{/if}
{#if editing}
  {@const t = editing.track}
  <TagEditor
    title="Edit tags"
    trackIds={[t.id]}
    fields={songFields}
    initial={{
      title: t.title,
      artist: t.artist ?? '',
      album: t.album ?? '',
      album_artist: t.album_artist ?? '',
      year: t.year?.toString() ?? '',
      genre: t.genre ?? tags?.genres.join(', ') ?? '',
    }}
    edited={songFields.filter((f) => editing?.edited[f] != null)}
    onclose={() => (editing = null)}
    onsaved={() => (editing = null)}
  />
{/if}
<AddToPlaylist kind="track" refId={track.id} label={track.title} trigger={false} bind:open={playlist} />

<style>
  .tags {
    display: grid;
    grid-template-columns: max-content 1fr;
    align-items: center;
    gap: 8px 12px;
    padding: 4px 4px 12px;
    border-bottom: 1px solid var(--line);
  }

  .styles {
    display: grid;
    gap: 6px;
  }

  .styles p {
    margin: 0;
  }

  .link {
    padding: 0;
    min-height: 0;
    border: 0;
    background: none;
    color: var(--accent);
    font-size: 0.8125rem;
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
