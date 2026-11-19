<script lang="ts">
  // Admin > User Guide: the guide that ships beside the program (`guide/`,
  // served by /api/guide), page by page. Links between pages open here;
  // links elsewhere open in a new tab.
  import { onMount, tick } from 'svelte'
  import { locale, t } from './i18n.svelte'
  import { markdown } from './markdown'
  import { marked, search, sections, words, type GuideSection } from './guideSearch'

  const ONLINE = 'https://github.com/jmarasch/DJINN/blob/main/docs/guide/README.md'

  let installed = $state<boolean | null>(null)
  let page = $state('README.md')
  let html = $state('')
  let failed = $state(false)
  let body = $state<HTMLElement>()
  let article = $state<HTMLElement>()
  let query = $state('')
  /** Every section of every page, read the first time someone searches. */
  let index = $state<GuideSection[] | null>(null)
  let indexing = false
  /** Words to mark on the page a result opened. */
  let highlight: string[] = []
  const hits = $derived(index && query.trim() ? search(index, query) : [])
  const wanted = $derived(words(query))

  /** Reads every page the contents page links to, once. */
  async function buildIndex() {
    if (index || indexing) return
    indexing = true
    try {
      const readme = await (await fetch(fileUrl('README.md'))).text()
      const pages = ['README.md', ...new Set([...readme.matchAll(/\]\(([\w-]+\.md)\)/g)].map((m) => m[1]))]
      const texts = await Promise.all(
        pages.map(async (name) => {
          const res = await fetch(fileUrl(name))
          return res.ok ? sections(name, await res.text()) : []
        }),
      )
      index = texts.flat()
    } catch {
      index = []
    } finally {
      indexing = false
    }
  }

  function openHit(section: GuideSection) {
    highlight = words(query)
    query = ''
    void open(section.page, section.anchor)
  }

  /** Marks the searched words on the page just opened. */
  function markWords() {
    if (!article || !highlight.length) return
    const walker = document.createTreeWalker(article, NodeFilter.SHOW_TEXT)
    const nodes: Text[] = []
    while (walker.nextNode()) nodes.push(walker.currentNode as Text)
    for (const node of nodes) {
      const pieces = marked(node.data, highlight)
      if (pieces.length < 2 && !pieces[0]?.hit) continue
      const frag = document.createDocumentFragment()
      for (const piece of pieces) {
        if (piece.hit) {
          const mark = document.createElement('mark')
          mark.textContent = piece.text
          frag.append(mark)
        } else frag.append(piece.text)
      }
      node.replaceWith(frag)
    }
    highlight = []
  }

  /** The address a guide file is fetched from. */
  const fileUrl = (name: string) =>
    name.startsWith('images/') ? `/api/guide/images/${encodeURIComponent(name.slice(7))}` : `/api/guide/${encodeURIComponent(name)}`

  async function open(name: string, anchor = '') {
    failed = false
    try {
      const res = await fetch(fileUrl(name))
      if (!res.ok) throw new Error(String(res.status))
      const text = await res.text()
      // Pages in images/ (the picture credits) point at pictures beside them.
      const base = name.startsWith('images/') ? '' : 'images/'
      html = markdown(text, { image: (src) => (src.startsWith('images/') ? fileUrl(src) : fileUrl(base + src)) })
      page = name
      await tick()
      markWords()
      if (anchor) body?.querySelector(`[id="${CSS.escape(anchor)}"]`)?.scrollIntoView()
      else body?.scrollIntoView({ block: 'start' })
    } catch {
      failed = true
    }
  }

  /** Follows a link inside the guide: another page, or a place on this one. */
  function follow(event: MouseEvent) {
    const link = (event.target as HTMLElement).closest('a')
    const href = link?.getAttribute('href')
    if (!link || !href || link.target === '_blank') return
    event.preventDefault()
    const [file, anchor = ''] = href.split('#')
    if (!file) {
      body?.querySelector(`[id="${CSS.escape(anchor)}"]`)?.scrollIntoView({ behavior: 'smooth' })
      return
    }
    if (!/^[\w./-]+\.md$/.test(file)) return
    const inImages = page.startsWith('images/')
    const name = file.startsWith('../') ? file.slice(3) : inImages && !file.includes('/') ? `images/${file}` : file
    void open(name, anchor)
  }

  onMount(async () => {
    try {
      const res = await fetch('/api/guide')
      installed = res.ok && ((await res.json()) as { installed: boolean }).installed
    } catch {
      installed = false
    }
    if (installed) await open('README.md')
  })
</script>

<section class="card guide" bind:this={body}>
  {#if installed === false}
    <p>{t('guide.not_installed')}</p>
    <p><a href={ONLINE} target="_blank" rel="noopener noreferrer">{t('guide.online')}</a></p>
  {:else if installed}
    <div class="bar">
      <input
        class="search"
        type="search"
        placeholder={t('guide.search')}
        aria-label={t('guide.search')}
        bind:value={query}
        onfocus={buildIndex}
        oninput={buildIndex}
      />
      {#if page !== 'README.md'}
        <button class="pill" onclick={() => open('README.md')}>{t('guide.contents')}</button>
      {/if}
      {#if !locale.code.startsWith('en')}
        <span class="small muted">{t('guide.english_only')}</span>
      {/if}
    </div>
    {#if query.trim()}
      {#if !index}
        <p class="muted">{t('common.loading')}</p>
      {:else if !hits.length}
        <p class="muted">{t('guide.no_results')}</p>
      {:else}
        <ul class="hits">
          {#each hits as hit (hit.section.page + hit.section.anchor + hit.section.heading)}
            <li>
              <button class="hit" onclick={() => openHit(hit.section)}>
                <strong>
                  {#each marked(hit.section.heading || hit.section.pageTitle, wanted) as piece, i (i)}
                    {#if piece.hit}<mark>{piece.text}</mark>{:else}{piece.text}{/if}
                  {/each}
                </strong>
                {#if hit.section.heading}<span class="small muted">{hit.section.pageTitle}</span>{/if}
                <span class="small">
                  {#each marked(hit.snippet, wanted) as piece, i (i)}
                    {#if piece.hit}<mark>{piece.text}</mark>{:else}{piece.text}{/if}
                  {/each}
                </span>
              </button>
            </li>
          {/each}
        </ul>
      {/if}
    {:else if failed}
      <p class="error">{t('guide.load_failed')}</p>
    {:else}
      <!-- The renderer shows any HTML in the guide as text, so this only holds what it made. -->
      <!-- eslint-disable-next-line svelte/no-at-html-tags -->
      <article class="page" lang="en" onclick={follow} role="presentation" bind:this={article}>{@html html}</article>
    {/if}
  {/if}
</section>

<style>
  .bar {
    display: flex;
    align-items: center;
    gap: 12px;
    flex-wrap: wrap;
    margin-bottom: 8px;
  }

  .search {
    flex: 1 1 260px;
    max-width: 420px;
  }

  .hits {
    list-style: none;
    padding: 0;
    margin: 0;
    max-width: 78ch;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .hit {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 2px;
    width: 100%;
    text-align: left;
    padding: 10px 12px;
    border-radius: 10px;
    border: 1px solid var(--edge);
    background: none;
    color: inherit;
    font: inherit;
    cursor: pointer;
  }

  .hit:hover,
  .hit:focus-visible {
    border-color: var(--edge-strong);
  }

  .guide :global(mark) {
    background: color-mix(in srgb, var(--accent) 35%, transparent);
    color: inherit;
    border-radius: 3px;
    padding: 0 1px;
  }

  .page {
    max-width: 78ch;
    line-height: 1.6;
    overflow-wrap: anywhere;
  }

  .page :global(h1) {
    font-size: 1.8rem;
    margin: 0.2em 0 0.6em;
  }

  .page :global(h2) {
    font-size: 1.35rem;
    margin: 1.6em 0 0.5em;
  }

  .page :global(h3) {
    font-size: 1.1rem;
    margin: 1.3em 0 0.4em;
  }

  .page :global(ul),
  .page :global(ol) {
    padding-left: 1.4em;
  }

  .page :global(li) {
    margin: 0.25em 0;
  }

  .page :global(a) {
    color: var(--accent);
  }

  .page :global(code) {
    font-size: 0.92em;
    padding: 0.1em 0.35em;
    border-radius: 6px;
    background: var(--card-bg, rgba(255, 255, 255, 0.06));
    border: 1px solid var(--edge);
  }

  .page :global(pre) {
    padding: 12px 14px;
    border-radius: 10px;
    border: 1px solid var(--edge);
    overflow-x: auto;
  }

  .page :global(pre code) {
    border: none;
    padding: 0;
    background: none;
  }

  .page :global(img) {
    display: block;
    max-width: 100%;
    height: auto;
    margin: 12px 0;
    border-radius: 12px;
    border: 1px solid var(--edge);
  }

  .page :global(.table) {
    overflow-x: auto;
  }

  .page :global(table) {
    border-collapse: collapse;
    margin: 12px 0;
  }

  .page :global(th),
  .page :global(td) {
    text-align: left;
    padding: 6px 10px;
    border-bottom: 1px solid var(--edge);
    vertical-align: top;
  }

  .page :global(blockquote) {
    margin: 12px 0;
    padding-left: 12px;
    border-left: 3px solid var(--edge-strong);
  }
</style>
