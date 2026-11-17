<script lang="ts">
  // Admin > Library > Genres: similar genres grouped into containers, so
  // "black metal", "death metal" and "speed metal" show as "Metal". Off by
  // default; the library's own genres are never changed. Auto-group sorts
  // them by how related they are (the slider says about how many
  // containers), and genres are dragged (or tapped, then "Move here")
  // between containers by hand.
  import { onMount } from 'svelte'
  import { api } from './api'
  import { loadGenreFold } from './genreFold.svelte'
  import { toast } from './state.svelte'
  import type { GenreGroupChoice, GenreGroups } from './types'

  let groups = $state<GenreGroups | null>(null)
  let choices = $state<GenreGroupChoice[] | null>(null)
  let pick = $state(0)
  let newName = $state('')
  let filter = $state('')
  let showAll = $state(false)
  let busy = $state(false)
  /** A genre tapped to move, for screens without dragging. */
  let moving = $state<string | null>(null)
  /** The container (or 'loose') a dragged genre is over. */
  let over = $state<number | 'loose' | null>(null)

  const LOOSE_SHOWN = 150
  const n = (x: number) => x.toLocaleString()

  async function load() {
    try {
      groups = await api.genreGroups()
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not load the genres', true)
    }
  }

  async function loadChoices() {
    try {
      choices = (await api.genreGroupChoices()).choices
      // Start in the middle: neither a few huge containers nor many tiny ones.
      pick = Math.floor(choices.length / 2)
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not work out the groups', true)
    }
  }

  onMount(() => {
    void load()
    void loadChoices()
  })

  /** Runs a change, then shows the result everywhere. */
  async function change(run: () => Promise<unknown>, done?: string) {
    busy = true
    try {
      await run()
      if (done) toast(done)
    } catch (e) {
      toast(e instanceof Error ? e.message : 'That did not work', true)
    } finally {
      busy = false
    }
    await load()
    void loadGenreFold(true)
  }

  const chosen = $derived(choices?.[pick] ?? null)
  const autoMade = $derived(groups?.containers.filter((c) => c.auto).length ?? 0)

  async function autoGroup() {
    if (!chosen) return
    if (
      autoMade &&
      !confirm(
        `Replace the ${autoMade} containers Auto-group made? Containers you made or changed, and genres you placed, stay.`,
      )
    )
      return
    await change(async () => {
      const made = await api.autoGenreGroups(chosen.containers)
      toast(`Made ${made.containers} containers holding ${made.grouped} genres`)
    })
    await loadChoices()
  }

  async function create() {
    const name = newName.trim()
    if (!name) return
    newName = ''
    await change(() => api.createGenreGroup(name))
  }

  async function rename(id: number, name: string, was: string) {
    if (!name.trim() || name.trim() === was) return
    await change(() => api.renameGenreGroup(id, name.trim()))
  }

  async function remove(id: number, name: string) {
    if (!confirm(`Remove the container ${name}? Its genres go back to being on their own.`)) return
    await change(() => api.deleteGenreGroup(id))
    await loadChoices()
  }

  async function moveTo(genre: string, group: number | null) {
    moving = null
    await change(() => api.placeGenre(genre, group))
  }

  async function clearAll() {
    if (!confirm('Remove every container, including ones you made?')) return
    await change(() => api.clearGenreGroups(), 'Containers removed')
    await loadChoices()
  }

  function dragStart(e: DragEvent, genre: string) {
    e.dataTransfer?.setData('text/plain', genre)
    if (e.dataTransfer) e.dataTransfer.effectAllowed = 'move'
  }

  function drop(e: DragEvent, group: number | null) {
    e.preventDefault()
    over = null
    const genre = e.dataTransfer?.getData('text/plain')
    if (genre) void moveTo(genre, group)
  }

  function allowDrop(e: DragEvent, target: number | 'loose') {
    e.preventDefault()
    over = target
  }

  const loose = $derived.by(() => {
    const all = groups?.loose ?? []
    const q = filter.trim().toLowerCase()
    return q ? all.filter((g) => g.genre.includes(q)) : all
  })
  const looseShown = $derived(showAll || filter.trim() ? loose : loose.slice(0, LOOSE_SHOWN))
</script>

{#snippet chip(genre: string, songs: number, pinned: boolean, inside: number | null)}
  <span
    class="chip"
    class:moving={moving === genre}
    draggable="true"
    role="button"
    tabindex="0"
    title="Drag to another container, or tap and choose Move here"
    ondragstart={(e) => dragStart(e, genre)}
    onclick={() => (moving = moving === genre ? null : genre)}
    onkeydown={(e) => {
      if (e.key === 'Enter' || e.key === ' ') {
        e.preventDefault()
        moving = moving === genre ? null : genre
      }
    }}
  >
    {genre}<span class="songs">{n(songs)}</span>
    {#if pinned && inside === null}
      <button
        class="x"
        title="Placed by hand. Let Auto-group place it again"
        aria-label="Let Auto-group place {genre}"
        onclick={(e) => {
          e.stopPropagation()
          void change(() => api.freeGenre(genre))
        }}>📌</button
      >
    {/if}
    {#if inside !== null}
      <button
        class="x"
        title="Take it out of this container"
        aria-label="Take {genre} out"
        onclick={(e) => {
          e.stopPropagation()
          void moveTo(genre, null)
        }}>×</button
      >
    {/if}
  </span>
{/snippet}

<section class="card wide">
  <h2>Genre containers</h2>
  <p class="small muted">
    Group similar genres into containers, so "black metal", "death metal" and "speed metal" all show as "Metal". While
    this is on, genre lists, the genre cloud and genre pages show the containers instead of the genres in them, and a
    container's page has every song of its genres. Artist, album and song pages still show the real genres, with the
    container first in pink. Your music files and the genres DJINN knows aren't changed; turn this off to put
    everything back.
  </p>
  {#if groups}
    <label class="switch">
      <input
        type="checkbox"
        checked={groups.enabled}
        disabled={busy}
        onchange={(e) => change(() => api.setGenreGroupsOn(e.currentTarget.checked))}
      />
      Show genres in their containers
    </label>

    <h3>Auto-group</h3>
    {#if choices === null}
      <p class="small muted">Working out how the genres relate...</p>
    {:else if choices.length === 0}
      <p class="small muted">
        Not enough related genres to group yet. Genres relate when songs share them or sound alike, so this works
        better after a library check and sound analysis.
      </p>
    {:else}
      <p class="small muted">
        Genres are grouped by how related they are: songs they share, and how alike their songs sound. Slide left for
        fewer, broader containers, right for more, closer ones. Genres you placed by hand and containers you made or
        changed are left as they are.
      </p>
      <label class="range">
        <span>
          About <strong>{chosen?.containers ?? 0}</strong> containers, holding {n(chosen?.grouped ?? 0)} genres
        </span>
        <input type="range" min="0" max={choices.length - 1} step="1" bind:value={pick} aria-label="How many containers" />
        <span class="ends small muted"><span>Broader</span><span>Closer</span></span>
      </label>
      <div class="actions">
        <button class="small-button" disabled={busy || !chosen} onclick={autoGroup}>Auto-group</button>
      </div>
    {/if}

    <h3>Containers</h3>
    <form
      class="new"
      onsubmit={(e) => {
        e.preventDefault()
        void create()
      }}
    >
      <input bind:value={newName} placeholder="A new container, like Metal" aria-label="New container name" maxlength="60" />
      <button class="small-button" type="submit" disabled={busy || !newName.trim()}>Make it</button>
    </form>
    {#if moving}
      <p class="small moving-note">
        Moving <strong>{moving}</strong>: choose Move here on a container.
        <button class="link" onclick={() => (moving = null)}>Cancel</button>
      </p>
    {/if}
    {#if groups.containers.length}
      <div class="grid">
        {#each groups.containers as c (c.id)}
          <div
            class="box"
            class:over={over === c.id}
            role="group"
            aria-label="Container {c.name}"
            ondragover={(e) => allowDrop(e, c.id)}
            ondragleave={() => (over = null)}
            ondrop={(e) => drop(e, c.id)}
          >
            <div class="head">
              <input
                class="name"
                value={c.name}
                aria-label="Container name"
                maxlength="60"
                onchange={(e) => rename(c.id, e.currentTarget.value, c.name)}
              />
              {#if c.auto}<span class="tag small muted" title="Made by Auto-group">auto</span>{/if}
              <button class="x" aria-label="Remove container {c.name}" onclick={() => remove(c.id, c.name)}>×</button>
            </div>
            <div class="chips">
              {#each c.genres as g (g.genre)}{@render chip(g.genre, g.songs, g.pinned, c.id)}{/each}
              {#if !c.genres.length}<span class="small muted">Drag genres here</span>{/if}
            </div>
            {#if moving && !c.genres.some((g) => g.genre === moving)}
              <button class="small-button here" onclick={() => moving && moveTo(moving, c.id)}>Move here</button>
            {/if}
          </div>
        {/each}
      </div>
    {:else}
      <p class="small muted">No containers yet. Use Auto-group, or make one and drag genres into it.</p>
    {/if}

    <div
      class="box loose"
      class:over={over === 'loose'}
      role="group"
      aria-label="Genres on their own"
      ondragover={(e) => allowDrop(e, 'loose')}
      ondragleave={() => (over = null)}
      ondrop={(e) => drop(e, null)}
    >
      <div class="head">
        <strong>On their own</strong>
        <span class="small muted">{n(groups.loose.length)} genres</span>
        <input class="filter" bind:value={filter} placeholder="Find a genre" aria-label="Find a genre" />
      </div>
      <div class="chips">
        {#each looseShown as g (g.genre)}{@render chip(g.genre, g.songs, g.pinned, null)}{/each}
      </div>
      {#if looseShown.length < loose.length}
        <button class="link" onclick={() => (showAll = true)}>Show all {n(loose.length)}</button>
      {/if}
      {#if moving && !groups.loose.some((g) => g.genre === moving)}
        <button class="small-button here" onclick={() => moving && moveTo(moving, null)}>Move here</button>
      {/if}
    </div>

    {#if groups.containers.length}
      <div class="actions">
        <button class="small-button danger" disabled={busy} onclick={clearAll}>Remove every container</button>
      </div>
    {/if}
  {:else}
    <p class="small muted">Loading...</p>
  {/if}
</section>

<style>
  .wide {
    grid-column: 1 / -1;
  }

  h3 {
    margin: 16px 0 4px;
    font-size: 0.95rem;
  }

  .range {
    display: grid;
    gap: 4px;
    margin: 8px 0;
  }

  .range input {
    width: 100%;
  }

  .ends {
    display: flex;
    justify-content: space-between;
  }

  .new {
    display: flex;
    gap: 8px;
    margin-bottom: 10px;
  }

  .new input {
    flex: 1;
    min-width: 0;
  }

  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(240px, 1fr));
    gap: 10px;
  }

  .box {
    padding: 10px;
    border: 1px solid var(--edge);
    border-radius: 14px;
    background: var(--surface);
  }

  .box.over {
    border-color: var(--ai-made);
    box-shadow: 0 0 10px color-mix(in srgb, var(--ai-made) 40%, transparent);
  }

  .loose {
    margin-top: 12px;
  }

  .head {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 8px;
  }

  .name {
    flex: 1;
    min-width: 0;
    color: var(--ai-made);
    font-weight: 600;
  }

  .filter {
    flex: 1;
    min-width: 0;
    max-width: 240px;
    margin-left: auto;
  }

  .tag {
    padding: 0 6px;
    border: 1px solid var(--edge);
    border-radius: 999px;
  }

  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .chip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 3px 4px 3px 10px;
    border-radius: 999px;
    background: var(--surface-2);
    font-size: 0.8125rem;
    cursor: grab;
    user-select: none;
  }

  .chip.moving {
    outline: 2px solid var(--ai-made);
  }

  .songs {
    color: var(--muted);
    font-size: 0.75rem;
  }

  .x {
    min-width: 0;
    min-height: 0;
    padding: 0 6px;
    border: 0;
    background: none;
    color: var(--muted);
  }

  .x:hover {
    color: var(--text);
  }

  .here {
    margin-top: 8px;
  }

  .moving-note {
    color: var(--ai-made);
  }

  .actions {
    margin-top: 12px;
  }
</style>
