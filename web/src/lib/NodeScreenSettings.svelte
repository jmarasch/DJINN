<script lang="ts">
  // What a screen plugged into a node shows: its own status, nothing, or
  // pages tried in order (timed lyrics, the node's visualizer, the TV page),
  // and how the visualizer looks. In Admin > Nodes and on the node's page.
  let {
    screen,
    visual,
    onchange,
  }: {
    /** 'status', 'off', or pages joined by commas. */
    screen: string
    /** 'bars', 'ring' or 'mirror'. */
    visual: string
    onchange: (screen: string, visual: string) => void
  } = $props()

  type Page = 'lyrics' | 'visual' | 'tv'
  const PAGES: { page: Page; label: string; note: string }[] = [
    { page: 'lyrics', label: 'Timed lyrics', note: 'while the song has timed lyrics' },
    { page: 'visual', label: 'Visualizer', note: 'drawn by this node from what its speaker plays' },
    { page: 'tv', label: 'TV page', note: "what's playing and the queue; always works" },
  ]
  const showsPages = $derived(screen !== 'status' && screen !== 'off')
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
    On a screen plugged into this box
    <select
      value={showsPages ? 'pages' : screen}
      onchange={(e) => {
        const value = e.currentTarget.value
        setScreen(value === 'pages' ? 'lyrics,visual,tv' : value)
      }}
    >
      <option value="pages">Lyrics, visualizer or TV page, in an order</option>
      <option value="status">This node's status</option>
      <option value="off">Nothing (blank screen)</option>
    </select>
  </label>
  {#if showsPages}
    <p class="small muted">The screen shows the first of these that can show now, and switches as songs change.</p>
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
              <button class="pill" disabled={at === 0} onclick={() => move(row.page, -1)} title="Try earlier">Up</button>
              <button class="pill" disabled={at === order.length - 1} onclick={() => move(row.page, 1)} title="Try later"
                >Down</button
              >
            </span>
          {/if}
        </li>
      {/each}
    </ol>
    {#if order.includes('visual')}
      <label>
        Visualizer style
        <select
          value={visual}
          onchange={(e) => onchange(screen, e.currentTarget.value)}
        >
          <option value="bars">Bars</option>
          <option value="ring">Ring</option>
          <option value="mirror">Mirror</option>
        </select>
      </label>
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
