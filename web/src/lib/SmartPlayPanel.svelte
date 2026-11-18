<script lang="ts">
  import { api } from './api'
  import Icon from './Icon.svelte'
  import { toast } from './state.svelte'
  import { locale, t } from './i18n.svelte'
  import type { Learned, PartyStyle, SmartPlaySettings } from './types'

  // Which cards: the smart play settings, or the party styles.
  let { part = 'play' }: { part?: 'play' | 'styles' } = $props()

  let settings = $state<SmartPlaySettings | null>(null)
  let mood = $state<{ on: boolean; strength: number } | null>(null)
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
    mood = data.mood
  }

  let moodTimer: ReturnType<typeof setTimeout>
  function saveMood() {
    clearTimeout(moodTimer)
    moodTimer = setTimeout(async () => {
      if (!mood) return
      try {
        await api.saveMoodSettings(mood)
      } catch (e) {
        toast(e instanceof Error ? e.message : t('common.save_failed_short'), true)
      }
    }, 200)
  }

  async function attempt(action: () => Promise<unknown>, done?: string) {
    try {
      await action()
      if (done) toast(done)
      await load()
      return true
    } catch (e) {
      toast(e instanceof Error ? e.message : t('common.failed'), true)
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
        toast(e instanceof Error ? e.message : t('common.save_failed_short'), true)
      }
    }, 200)
  }

  async function add(e: SubmitEvent) {
    e.preventDefault()
    const name = newName.trim()
    if (await attempt(() => api.createPartyStyle(name), t('sp.added', { name }))) newName = ''
  }

  function useStyle(style: PartyStyle) {
    attempt(() => api.usePartyStyle(style.id), t('sp.now_learning', { name: style.name }))
  }

  function rename(style: PartyStyle) {
    const name = prompt(t('sp.rename_q'), style.name)?.trim()
    if (name && name !== style.name) attempt(() => api.renamePartyStyle(style.id, name))
  }

  async function clear(style: PartyStyle) {
    if (!confirm(t('sp.clear_q', { name: style.name }))) return
    if (await attempt(() => api.clearPartyStyle(style.id), t('sp.fresh', { name: style.name }))) {
      if (showing === style.id) learned = await api.partyLearned(style.id)
    }
  }

  async function remove(style: PartyStyle) {
    if (!confirm(t('sp.delete_q', { name: style.name }))) return
    if (await attempt(() => api.deletePartyStyle(style.id), t('sp.deleted', { name: style.name }))) {
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
      toast(e instanceof Error ? e.message : t('common.load_failed'), true)
    }
  }

  const strengthLabel = (n: number) =>
    n === 0 ? t('out.off') : n < 40 ? t('sp.gentle') : n < 75 ? t('sp.medium') : t('sp.strong')
  /** "2.1x" for a weight, as how much more or less often it comes up. */
  const factor = (w: number) =>
    `${w.toLocaleString(locale.code, { maximumFractionDigits: w >= 10 ? 0 : 1, minimumFractionDigits: w >= 10 ? 0 : 1 })}x`
  const nothingLearned = $derived(
    learned !== null &&
      [learned.songs, learned.artists, learned.genres].every((e) => !e.favored.length && !e.avoided.length),
  )
</script>

{#if settings}
  {#if part === 'play'}
  <section class="card">
    <h2>{t('sp.title')}</h2>
    <p class="small muted">
      {t('sp.intro')}
    </p>
    <label class="switch">
      <input type="checkbox" bind:checked={settings.smart_play} onchange={save} />
      {t('sp.learn')}
    </label>
    <label class="switch">
      <input type="checkbox" bind:checked={settings.smart_playlists} onchange={save} disabled={!settings.smart_play} />
      {t('sp.playlists')}
    </label>
    <label class="slider">
      <span class="row-label">
        {t('sp.strength')} <span class="muted">{strengthLabel(settings.strength)} ({settings.strength}%)</span>
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

  {#if mood}
    <section class="card">
      <h2>{t('sp.mood')}</h2>
      <p class="small muted">
        {t('sp.mood_note')}
      </p>
      <label class="switch">
        <input type="checkbox" bind:checked={mood.on} onchange={saveMood} />
        {t('sp.mood_on')}
      </label>
      <label class="slider">
        <span class="row-label">
          {t('sp.mood_strength')} <span class="muted">{strengthLabel(mood.strength)} ({mood.strength}%)</span>
        </span>
        <input type="range" min="0" max="100" step="5" bind:value={mood.strength} oninput={saveMood} disabled={!mood.on} />
      </label>
    </section>
  {/if}
  {/if}

  {#if part === 'styles'}
  <section class="card">
    <h2>{t('sp.styles')}</h2>
    <p class="small muted">
      {t('sp.styles_note')}
    </p>
    {#each styles as style (style.id)}
      <div class="style">
        <div class="grow">
          <div>
            {style.name}
            {#if style.id === active}<span class="badge">{t('sp.in_use')}</span>{/if}
          </div>
          <div class="small muted">{t('sp.requests', { n: style.requests })} · {t('count.plays', { n: style.plays })}</div>
          <div class="buttons">
            {#if style.id !== active}
              <button class="small-button primary" onclick={() => useStyle(style)}>{t('sp.use_now')}</button>
            {/if}
            <button class="small-button" aria-expanded={showing === style.id} onclick={() => toggleLearned(style)}>
              {showing === style.id ? t('sp.hide_learned') : t('sp.learned')}
            </button>
            <button class="small-button" onclick={() => rename(style)}>{t('common.rename')}</button>
            <button class="small-button" disabled={!style.requests && !style.plays} onclick={() => clear(style)}>
              {t('sp.clear')}
            </button>
          </div>
        </div>
        {#if style.id !== active}
          <button class="icon-only" aria-label={t('sp.delete', { name: style.name })} onclick={() => remove(style)}>
            <Icon name="close" />
          </button>
        {/if}
      </div>
      {#if showing === style.id && learned}
        <div class="learned">
          {#if nothingLearned}
            <p class="small muted">{t('sp.nothing')}</p>
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
            {#each [[t('sp.favored'), 'favored'], [t('sp.avoided'), 'avoided']] as const as [heading, side] (side)}
              {#if learned.songs[side].length || learned.artists[side].length || learned.genres[side].length}
                <h3>{heading}</h3>
                {@render list(
                  t('sp.songs'),
                  learned.songs[side],
                  (s) => `${s.track.title}${s.track.artist ? ` · ${s.track.artist}` : ''}`,
                  (s) => s.weight,
                )}
                {@render list(t('sp.artists'), learned.artists[side], (a) => a.artist.name, (a) => a.weight)}
                {@render list(t('sp.genres'), learned.genres[side], (g) => g.name, (g) => g.weight)}
              {/if}
            {/each}
            <p class="small muted">
              {t('sp.number_note')}
            </p>
          {/if}
        </div>
      {/if}
    {/each}
    <form class="new-style" onsubmit={add}>
      <input placeholder={t('sp.new_style')} bind:value={newName} maxlength="40" required />
      <button class="primary">{t('common.add')}</button>
    </form>
  </section>
  {/if}
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
