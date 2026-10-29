<script lang="ts">
  import { api } from './api'
  import Icon from './Icon.svelte'
  import { toast } from './state.svelte'
  import type { Learned, PartyStyle, SmartPlaySettings } from './types'

  let settings = $state<SmartPlaySettings | null>(null)
  let styles = $state<PartyStyle[]>([])
  let active = $state<number | null>(null)
  let newName = $state('')
  /** The style whose learning is shown, and what it learned. */
  let showing = $state<number | null>(null)
  let learned = $state<Learned | null>(null)

  $effect(() => {
    load().catch((e) => toast(e.message, true))
  })

  async function load() {
    const data = await api.smartPlay()
    settings = { smart_play: data.smart_play, smart_playlists: data.smart_playlists, strength: data.strength }
    styles = data.styles
    active = data.active
  }

  async function attempt(action: () => Promise<unknown>, done?: string) {
    try {
      await action()
      if (done) toast(done)
      await load()
      return true
    } catch (e) {
      toast(e instanceof Error ? e.message : 'That did not work', true)
      return false
    }
  }

  let timer: ReturnType<typeof setTimeout>
  function save() {
    clearTimeout(timer)
    timer = setTimeout(async () => {
      if (!settings) return
      try {
        await api.saveSmartPlay(settings)
      } catch (e) {
        toast(e instanceof Error ? e.message : 'Could not save', true)
      }
    }, 200)
  }

  async function add(e: SubmitEvent) {
    e.preventDefault()
    const name = newName.trim()
    if (await attempt(() => api.createPartyStyle(name), `Added ${name}`)) newName = ''
  }

  function useStyle(style: PartyStyle) {
    attempt(() => api.usePartyStyle(style.id), `Now learning as ${style.name}`)
  }

  function rename(style: PartyStyle) {
    const name = prompt('New name for this style:', style.name)?.trim()
    if (name && name !== style.name) attempt(() => api.renamePartyStyle(style.id, name))
  }

  async function clear(style: PartyStyle) {
    if (!confirm(`Forget what ${style.name} parties liked? The request log keeps its history.`)) return
    if (await attempt(() => api.clearPartyStyle(style.id), `${style.name} starts fresh`)) {
      if (showing === style.id) learned = await api.partyLearned(style.id)
    }
  }

  async function remove(style: PartyStyle) {
    if (!confirm(`Delete ${style.name} and what it learned?`)) return
    if (await attempt(() => api.deletePartyStyle(style.id), `Deleted ${style.name}`)) {
      if (showing === style.id) showing = null
    }
  }

  async function toggleLearned(style: PartyStyle) {
    if (showing === style.id) {
      showing = null
      return
    }
    try {
      learned = await api.partyLearned(style.id)
      showing = style.id
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not load it', true)
    }
  }

  const strengthLabel = (n: number) => (n === 0 ? 'Off' : n < 40 ? 'Gentle' : n < 75 ? 'Medium' : 'Strong')
  const times = (n: number, word: string) => `${n} ${word}${n === 1 ? '' : 's'}`
  /** "2.1x" for a weight, as how much more or less often it comes up. */
  const factor = (w: number) => `${w >= 10 ? w.toFixed(0) : w.toFixed(1)}x`
  const nothingLearned = $derived(
    learned !== null &&
      [learned.songs, learned.artists, learned.genres].every((e) => !e.favored.length && !e.avoided.length),
  )
</script>

{#if settings}
  <section class="card">
    <h2>Smart play</h2>
    <p class="small muted">
      When nobody has requested anything, DJINN picks songs itself. Smart play has it learn from the party: songs
      people request come up more, and so do others by the same artists and in the same genres. Songs people skip
      come up less.
    </p>
    <label class="switch">
      <input type="checkbox" bind:checked={settings.smart_play} onchange={save} />
      Learn from the party for wild play and radio
    </label>
    <label class="switch">
      <input type="checkbox" bind:checked={settings.smart_playlists} onchange={save} disabled={!settings.smart_play} />
      Also use it for shuffled playlists (every song still plays once before any repeats)
    </label>
    <label class="slider">
      <span class="row-label">
        How much it counts <span class="muted">{strengthLabel(settings.strength)} ({settings.strength}%)</span>
      </span>
      <input
        type="range"
        min="0"
        max="100"
        step="5"
        bind:value={settings.strength}
        oninput={save}
        disabled={!settings.smart_play}
      />
    </label>
  </section>

  <section class="card">
    <h2>Party styles</h2>
    <p class="small muted">
      Each style keeps its own memory, so a Halloween party doesn't change what plays at the family BBQ. Use the one
      that fits tonight: requests, songs played through, and skips count toward it.
    </p>
    {#each styles as style (style.id)}
      <div class="style">
        <div class="grow">
          <div>
            {style.name}
            {#if style.id === active}<span class="badge">In use</span>{/if}
          </div>
          <div class="small muted">{times(style.requests, 'request')} · {times(style.plays, 'play')}</div>
          <div class="buttons">
            {#if style.id !== active}
              <button class="small-button primary" onclick={() => useStyle(style)}>Use now</button>
            {/if}
            <button class="small-button" aria-expanded={showing === style.id} onclick={() => toggleLearned(style)}>
              {showing === style.id ? 'Hide what it learned' : 'What it learned'}
            </button>
            <button class="small-button" onclick={() => rename(style)}>Rename</button>
            <button class="small-button" disabled={!style.requests && !style.plays} onclick={() => clear(style)}>
              Clear history
            </button>
          </div>
        </div>
        {#if style.id !== active}
          <button class="icon-only" aria-label="Delete {style.name}" onclick={() => remove(style)}>
            <Icon name="close" />
          </button>
        {/if}
      </div>
      {#if showing === style.id && learned}
        <div class="learned">
          {#if nothingLearned}
            <p class="small muted">Nothing yet. It learns as people request and skip songs while this style is in use.</p>
          {:else}
            {#snippet list<T>(title: string, items: T[], name: (item: T) => string, weight: (item: T) => number)}
              {#if items.length}
                <h4>{title}</h4>
                <ol>
                  {#each items as item, i (i)}
                    <li>
                      <span class="ellipsis">{name(item)}</span>
                      <span class="muted small factor">{factor(weight(item))}</span>
                    </li>
                  {/each}
                </ol>
              {/if}
            {/snippet}
            {#each [['Favored', 'favored'], ['Avoided', 'avoided']] as const as [heading, side] (side)}
              {#if learned.songs[side].length || learned.artists[side].length || learned.genres[side].length}
                <h3>{heading}</h3>
                {@render list(
                  'Songs',
                  learned.songs[side],
                  (s) => `${s.track.title}${s.track.artist ? ` · ${s.track.artist}` : ''}`,
                  (s) => s.weight,
                )}
                {@render list('Artists', learned.artists[side], (a) => a.artist.name, (a) => a.weight)}
                {@render list('Genres', learned.genres[side], (g) => g.name, (g) => g.weight)}
              {/if}
            {/each}
            <p class="small muted">
              The number is how much more (or less) often it comes up. Artists and genres count for less than a song
              itself.
            </p>
          {/if}
        </div>
      {/if}
    {/each}
    <form class="new-style" onsubmit={add}>
      <input placeholder="New style, like Halloween" bind:value={newName} maxlength="40" required />
      <button class="primary">Add</button>
    </form>
  </section>
{/if}

<style>
  .slider {
    gap: 2px;
  }

  .row-label {
    display: flex;
    flex-direction: row;
    justify-content: space-between;
    font-size: 0.875rem;
  }

  .switch:has(input:disabled),
  .slider:has(input:disabled) {
    opacity: 0.5;
  }

  .style {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 10px 0;
    border-bottom: 1px solid var(--line);
  }

  .grow {
    flex: 1;
    min-width: 0;
  }

  .badge {
    margin-left: 6px;
    padding: 2px 8px;
    border-radius: 999px;
    background: var(--accent);
    color: var(--accent-text);
    font-size: 0.75rem;
    font-weight: 600;
  }

  .buttons {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin-top: 8px;
  }

  .learned {
    padding: 4px 0 12px;
    border-bottom: 1px solid var(--line);
  }

  .learned h3 {
    margin: 12px 0 4px;
    font-size: 0.9375rem;
  }

  .learned h4 {
    margin: 8px 0 2px;
    color: var(--muted);
    font-size: 0.8125rem;
    font-weight: 600;
  }

  .learned ol {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .learned li {
    display: flex;
    justify-content: space-between;
    gap: 10px;
    padding: 3px 0;
  }

  .factor {
    flex: none;
    font-variant-numeric: tabular-nums;
  }

  .new-style {
    display: flex;
    gap: 8px;
    margin-top: 12px;
  }

  .new-style input {
    flex: 1;
    min-width: 0;
  }
</style>
