<script lang="ts">
  // Admin > User Guide: the guide that ships beside the program (`guide/`,
  // served by /api/guide), page by page. Links between pages open here;
  // links elsewhere open in a new tab.
  import { onMount, tick } from 'svelte'
  import { locale, t } from './i18n.svelte'
  import { markdown } from './markdown'

  const ONLINE = 'https://github.com/jmarasch/DJINN/blob/main/docs/guide/README.md'

  let installed = $state<boolean | null>(null)
  let page = $state('README.md')
  let html = $state('')
  let failed = $state(false)
  let body = $state<HTMLElement>()

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
      {#if page !== 'README.md'}
        <button class="pill" onclick={() => open('README.md')}>{t('guide.contents')}</button>
      {/if}
      {#if !locale.code.startsWith('en')}
        <span class="small muted">{t('guide.english_only')}</span>
      {/if}
    </div>
    {#if failed}
      <p class="error">{t('guide.load_failed')}</p>
    {:else}
      <!-- The renderer shows any HTML in the guide as text, so this only holds what it made. -->
      <!-- eslint-disable-next-line svelte/no-at-html-tags -->
      <article class="page" lang="en" onclick={follow} role="presentation">{@html html}</article>
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
