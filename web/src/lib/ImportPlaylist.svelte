<script lang="ts">
  import { t } from './i18n.svelte'
  import { api } from './api'
  import Icon from './Icon.svelte'
  import { toast } from './state.svelte'
  import type { ImportedPlaylist } from './types'

  let { onimported }: { onimported: () => void } = $props()

  let open = $state(false)
  let name = $state('')
  let pasted = $state('')
  let busy = $state(false)
  let results = $state<ImportedPlaylist[]>([])
  let skipped = $state(0)
  let input = $state<HTMLInputElement>()
  let hasKey = $state(false)

  $effect(() => {
    if (open) {
      api
        .youtubeKey()
        .then((status) => (hasKey = status.has_key))
        .catch(() => {})
    }
  })

  /** Apple Music's text export is UTF-16; the rest are UTF-8. */
  async function readText(file: File) {
    const bytes = new Uint8Array(await file.arrayBuffer())
    const utf16 = (bytes[0] === 0xff && bytes[1] === 0xfe) || (bytes[0] === 0xfe && bytes[1] === 0xff)
    const big = bytes[0] === 0xfe
    return new TextDecoder(utf16 ? (big ? 'utf-16be' : 'utf-16le') : 'utf-8').decode(bytes)
  }

  async function run(items: { text: string; filename?: string }[]) {
    busy = true
    results = []
    skipped = 0
    try {
      for (const item of items) {
        const result = await api.importPlaylist(item.text, item.filename, items.length === 1 ? name : undefined)
        results.push(...result.playlists)
        skipped += result.skipped
      }
      name = ''
      pasted = ''
      onimported()
    } catch (e) {
      toast(e instanceof Error ? e.message : t('ip.failed'), true)
    } finally {
      busy = false
      if (input) input.value = ''
    }
  }

  async function pick() {
    const files = Array.from(input?.files ?? [])
    if (!files.length) return
    await run(await Promise.all(files.map(async (f) => ({ text: await readText(f), filename: f.name }))))
  }
</script>

{#if open}
  <div class="card panel">
    <div class="head">
      <h2>{t('ip.title')}</h2>
      <button class="icon" aria-label={t('common.close')} onclick={() => (open = false)}><Icon name="close" /></button>
    </div>
    <p class="muted small">
      {t('ip.intro')}
    </p>

    <input placeholder={t('ip.name')} bind:value={name} maxlength="60" />

    <input
      class="hidden"
      type="file"
      multiple
      accept=".csv,.tsv,.txt,.json,.jspf,.xspf,.xml,.m3u,.m3u8,.pls,text/*,application/json"
      bind:this={input}
      onchange={pick}
    />
    <button class="primary" disabled={busy} onclick={() => input?.click()}>
      <Icon name="upload" size={20} /> {busy ? t('ip.importing') : t('ip.choose')}
    </button>

    <textarea rows="4" placeholder={t('ip.paste_hint')} bind:value={pasted}
    ></textarea>
    <button disabled={busy || !pasted.trim()} onclick={() => run([{ text: pasted }])}>{t('ip.import_pasted')}</button>

    <details>
      <summary>{t('ip.how')}</summary>
      <ul class="small">
        <li>
          <b>{t('ip.yt_title')}</b> {t('ip.yt')}
        </li>
        <li>
          <b>{t('ip.others_title')}</b>
          <a href="https://www.tunemymusic.com/" target="_blank" rel="noreferrer">Tune My Music</a> {t('ip.others_1')}
          <b>{t('ip.export_to_file')}</b> {t('ip.others_2')}
        </li>
        <li>
          <b>{t('ip.spotify_title')}</b> <a href="https://exportify.net/" target="_blank" rel="noreferrer">Exportify</a>,
          {t('ip.spotify', { file: 'Playlist1.json' })}
        </li>
        <li><b>{t('ip.apple_title')}</b> {t('ip.apple')}</li>
        <li><b>{t('ip.players_title')}</b> {t('ip.players')}</li>
      </ul>
    </details>

    <p class="small muted">
      {hasKey ? t('ip.yt_on') : t('ip.yt_off')}{#if !hasKey}
        {t('ip.yt_admin')}{/if}
    </p>

    {#each results as result (result.id)}
      <a class="result" href="#/playlist/{result.id}">
        <span class="ellipsis">{result.name}</span>
        <span class="small muted">
          {t('ip.found', { n: result.found })}{#if result.missing}<b class="bad">{t('ip.not_in_lib', { n: result.missing })}</b>{/if}
        </span>
      </a>
    {/each}
    {#if skipped}
      <p class="muted small">{t('ip.skipped', { n: skipped })}</p>
    {/if}
  </div>
{:else}
  <button class="opener" onclick={() => (open = true)}><Icon name="upload" size={18} /> {t('ip.title')}</button>
{/if}

<style>
  .panel {
    gap: 10px;
    margin: 12px 0;
  }

  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .head h2 {
    margin: 0;
  }

  .icon {
    width: 40px;
    padding: 0;
    background: transparent;
    color: var(--muted);
  }

  .hidden {
    display: none;
  }

  textarea {
    width: 100%;
    box-sizing: border-box;
    resize: vertical;
    font: inherit;
  }

  ul {
    margin: 8px 0 0;
    padding-left: 18px;
  }

  li {
    margin-bottom: 6px;
  }

  .result {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 8px 0;
    border-top: 1px solid var(--line);
    color: var(--text);
    text-decoration: none;
  }

  .bad {
    color: var(--bad);
  }

  .opener {
    margin-bottom: 4px;
  }
</style>
