<script lang="ts">
  // What a screen plugged into a node shows: its own status, nothing, or
  // pages tried in order (timed lyrics, the node's visualizer, the TV page),
  // and its own visualizers if it shouldn't follow the house's. In Admin >
  // Nodes and on the node's page, where a node can also follow its room's
  // setting; and for a room's own setting, its screens' default.
  import { t } from './i18n.svelte'
  import { DEFAULT_VISUALS, type Visuals } from './visuals'
  import VisualsEditor from './VisualsEditor.svelte'

  let {
    screen,
    visual,
    start,
    canFollow = false,
    noVisual = false,
    onchange,
  }: {
    /** 'status', 'off', or pages joined by commas; '' follows the room. */
    screen: string
    /** The node's own visualizers, or null to follow the house's. */
    visual: Visuals | null
    /** Where its own start from when first made (the house's, if known). */
    start?: Visuals | null
    /** A node's setting: it can follow its room's instead of its own. */
    canFollow?: boolean
    /** A room's setting: visualizers are each node's own. */
    noVisual?: boolean
    onchange: (screen: string, visual: Visuals | null) => void
  } = $props()

  type Page = 'lyrics' | 'visual' | 'tv'
  const PAGES: { page: Page; label: string; note: string }[] = $derived([
    { page: 'lyrics', label: t('ni.page_lyrics'), note: t('nss.lyrics_note') },
    { page: 'visual', label: t('ni.page_visual'), note: t('nss.visual_note') },
    { page: 'tv', label: t('ni.page_tv'), note: t('nss.tv_note') },
  ])
  const following = $derived(canFollow && screen === '')
  const showsPages = $derived(!following && screen !== 'status' && screen !== 'off')
  /** The pages tried, in order, then the ones left out. */
  const order = $derived(showsPages ? (screen.split(',') as Page[]) : [])
  const rows = $derived([
    ...order.map((page) => PAGES.find((p) => p.page === page)).filter((p) => !!p),
    ...PAGES.filter((p) => !order.includes(p.page)),
  ])

  function setScreen(value: string) {
    onchange(value, visual)
  }

  function toggle(page: Page, on: boolean) {
    const next = on ? [...order, page] : order.filter((p) => p !== page)
    // At least one page; the TV page if every other is taken off.
    setScreen((next.length ? next : ['tv']).join(','))
  }

  function move(page: Page, by: number) {
    const at = order.indexOf(page)
    const to = at + by
    if (at < 0 || to < 0 || to >= order.length) return
    const next = [...order]
    next.splice(at, 1)
    next.splice(to, 0, page)
    setScreen(next.join(','))
  }
</script>

<div class="screen-settings">
  <label>
    {t('nss.on_screen')}
    <select
      value={following ? 'room' : showsPages ? 'pages' : screen}
      onchange={(e) => {
        const value = e.currentTarget.value
        setScreen(value === 'pages' ? 'lyrics,visual,tv' : value === 'room' ? '' : value)
      }}
    >
      {#if canFollow}<option value="room">{t('nss.room')}</option>{/if}
      <option value="pages">{t('nss.pages')}</option>
      <option value="status">{t('nss.status')}</option>
      <option value="off">{t('nss.off')}</option>
    </select>
  </label>
  {#if showsPages}
    <p class="small muted">{t('nss.first')}</p>
    <ol class="pages">
      {#each rows as row (row.page)}
        {@const at = order.indexOf(row.page)}
        <li class:off={at < 0}>
          <label class="check">
            <input type="checkbox" checked={at >= 0} onchange={(e) => toggle(row.page, e.currentTarget.checked)} />
            <span><strong>{row.label}</strong> <span class="small muted">{row.note}</span></span>
          </label>
          {#if at >= 0}
            <span class="moves">
              <button class="pill" disabled={at === 0} onclick={() => move(row.page, -1)} title={t('nss.earlier')}>{t('nss.up')}</button>
              <button class="pill" disabled={at === order.length - 1} onclick={() => move(row.page, 1)} title={t('nss.later')}
                >{t('nss.down')}</button
              >
            </span>
          {/if}
        </li>
      {/each}
    </ol>
    {#if order.includes('visual') && !noVisual}
      <label class="check">
        <input
          type="checkbox"
          checked={!visual}
          onchange={(e) => onchange(screen, e.currentTarget.checked ? null : structuredClone(start ?? DEFAULT_VISUALS))}
        />
        {t('nss.house')}
      </label>
      {#if visual}
        <p class="small muted">{t('nss.own')}</p>
        <VisualsEditor value={visual} onchange={(own) => onchange(screen, own)} />
      {/if}
    {/if}
  {/if}
</div>

<style>
  .screen-settings {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  label {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  label.check {
    flex-direction: row;
    align-items: center;
    gap: 8px;
  }
  .pages {
    margin: 0;
    padding: 0;
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .pages li {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 6px 10px;
    border-radius: 8px;
    background: var(--surface-2);
  }

  .pages li.off {
    opacity: 0.6;
  }

  .moves {
    display: flex;
    gap: 4px;
    flex: none;
  }
</style>
