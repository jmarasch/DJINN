<script lang="ts">
  // Admin > Genres > Containers: similar genres grouped into containers, so
  // "black metal", "death metal" and "speed metal" show as "Metal". Off by
  // default; the library's own genres are never changed. Auto-group sorts
  // them by how related they are (the slider says about how many
  // containers), and genres are dragged (or tapped, then "Move here")
  // between containers by hand.
  import { onMount } from 'svelte'
  import { api } from './api'
  import { loadGenreFold } from './genreFold.svelte'
  import { toast } from './state.svelte'
  import { locale, t } from './i18n.svelte'
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
  const n = (x: number) => x.toLocaleString(locale.code)

  async function load() {
    try {
      groups = await api.genreGroups()
    } catch (e) {
      toast(e instanceof Error ? e.message : t('gg.load_failed'), true)
    }
  }

  async function loadChoices() {
    try {
      choices = (await api.genreGroupChoices()).choices
      // Start in the middle: neither a few huge containers nor many tiny ones.
      pick = Math.floor(choices.length / 2)
    } catch (e) {
      toast(e instanceof Error ? e.message : t('gg.choices_failed'), true)
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
      toast(e instanceof Error ? e.message : t('common.failed'), true)
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
        t('gg.replace_q', { n: autoMade }),
      )
    )
      return
    await change(async () => {
      const made = await api.autoGenreGroups(chosen.containers)
      toast(t('gg.made', { containers: made.containers, genres: made.grouped }))
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
    if (!confirm(t('gg.remove_q', { name }))) return
    await change(() => api.deleteGenreGroup(id))
    await loadChoices()
  }

  async function moveTo(genre: string, group: number | null) {
    moving = null
    await change(() => api.placeGenre(genre, group))
  }

  async function clearAll() {
    if (!confirm(t('gg.clear_q'))) return
    await change(() => api.clearGenreGroups(), t('gg.removed'))
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
    title={t('gg.chip_title')}
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
        title={t('gg.pinned_title')}
        aria-label={t('gg.free', { genre })}
        onclick={(e) => {
          e.stopPropagation()
          void change(() => api.freeGenre(genre))
        }}>📌</button
      >
    {/if}
    {#if inside !== null}
      <button
        class="x"
        title={t('gg.take_out_title')}
        aria-label={t('gg.take_out', { genre })}
        onclick={(e) => {
          e.stopPropagation()
          void moveTo(genre, null)
        }}>×</button
      >
    {/if}
  </span>
{/snippet}

<section class="card wide">
  <h2>{t('gg.title')}</h2>
  <p class="small muted">
    {t('gg.intro')}
  </p>
  {#if groups}
    <label class="switch">
      <input
        type="checkbox"
        checked={groups.enabled}
        disabled={busy}
        onchange={(e) => change(() => api.setGenreGroupsOn(e.currentTarget.checked))}
      />
      {t('gg.on')}
    </label>

    <h3>{t('gg.auto')}</h3>
    {#if choices === null}
      <p class="small muted">{t('gg.working')}</p>
    {:else if choices.length === 0}
      <p class="small muted">
        {t('gg.not_enough')}
      </p>
    {:else}
      <p class="small muted">
        {t('gg.how')}
      </p>
      <label class="range">
        <span>
          {t('gg.about', { containers: chosen?.containers ?? 0, genres: n(chosen?.grouped ?? 0) })}
        </span>
        <input type="range" min="0" max={choices.length - 1} step="1" bind:value={pick} aria-label={t('gg.how_many')} />
        <span class="ends small muted"><span>{t('gg.broader')}</span><span>{t('gg.closer')}</span></span>
      </label>
      <div class="actions">
        <button class="small-button" disabled={busy || !chosen} onclick={autoGroup}>{t('gg.auto')}</button>
      </div>
    {/if}

    <h3>{t('gg.containers')}</h3>
    <form
      class="new"
      onsubmit={(e) => {
        e.preventDefault()
        void create()
      }}
    >
      <input bind:value={newName} placeholder={t('gg.new_hint')} aria-label={t('gg.new_name')} maxlength="60" />
      <button class="small-button" type="submit" disabled={busy || !newName.trim()}>{t('gg.make')}</button>
    </form>
    {#if moving}
      <p class="small moving-note">
        {t('gg.moving', { genre: moving })}
        <button class="link" onclick={() => (moving = null)}>{t('common.cancel')}</button>
      </p>
    {/if}
    {#if groups.containers.length}
      <div class="grid">
        {#each groups.containers as c (c.id)}
          <div
            class="box"
            class:over={over === c.id}
            role="group"
            aria-label={t('gg.container', { name: c.name })}
            ondragover={(e) => allowDrop(e, c.id)}
            ondragleave={() => (over = null)}
            ondrop={(e) => drop(e, c.id)}
          >
            <div class="head">
              <input
                class="name"
                value={c.name}
                aria-label={t('gg.container_name')}
                maxlength="60"
                onchange={(e) => rename(c.id, e.currentTarget.value, c.name)}
              />
              {#if c.auto}<span class="tag small muted" title={t('gg.made_by_auto')}>{t('gg.auto_tag')}</span>{/if}
              <button class="x" aria-label={t('gg.remove_container', { name: c.name })} onclick={() => remove(c.id, c.name)}>×</button>
            </div>
            <div class="chips">
              {#each c.genres as g (g.genre)}{@render chip(g.genre, g.songs, g.pinned, c.id)}{/each}
              {#if !c.genres.length}<span class="small muted">{t('gg.drag_here')}</span>{/if}
            </div>
            {#if moving && !c.genres.some((g) => g.genre === moving)}
              <button class="small-button here" onclick={() => moving && moveTo(moving, c.id)}>{t('gg.move_here')}</button>
            {/if}
          </div>
        {/each}
      </div>
    {:else}
      <p class="small muted">{t('gg.none')}</p>
    {/if}

    <div
      class="box loose"
      class:over={over === 'loose'}
      role="group"
      aria-label={t('gg.loose_label')}
      ondragover={(e) => allowDrop(e, 'loose')}
      ondragleave={() => (over = null)}
      ondrop={(e) => drop(e, null)}
    >
      <div class="head">
        <strong>{t('gg.loose')}</strong>
        <span class="small muted">{t('count.genres', { n: groups.loose.length })}</span>
        <input class="filter" bind:value={filter} placeholder={t('gg.find_genre')} aria-label={t('gg.find_genre')} />
      </div>
      <div class="chips">
        {#each looseShown as g (g.genre)}{@render chip(g.genre, g.songs, g.pinned, null)}{/each}
      </div>
      {#if looseShown.length < loose.length}
        <button class="link" onclick={() => (showAll = true)}>{t('gg.show_all', { n: n(loose.length) })}</button>
      {/if}
      {#if moving && !groups.loose.some((g) => g.genre === moving)}
        <button class="small-button here" onclick={() => moving && moveTo(moving, null)}>{t('gg.move_here')}</button>
      {/if}
    </div>

    {#if groups.containers.length}
      <div class="actions">
        <button class="small-button danger" disabled={busy} onclick={clearAll}>{t('gg.remove_all')}</button>
      </div>
    {/if}
  {:else}
    <p class="small muted">{t('common.loading')}</p>
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
