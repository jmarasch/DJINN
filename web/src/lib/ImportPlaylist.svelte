<script lang="ts">
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
      toast(e instanceof Error ? e.message : 'Could not import that', true)
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
      <h2>Import a playlist</h2>
      <button class="icon" aria-label="Close" onclick={() => (open = false)}><Icon name="close" /></button>
    </div>
    <p class="muted small">
      This brings in the list, not the music. Songs you don't have stay in the list, flagged, and are skipped. They
      start playing by themselves once they're in your library.
    </p>

    <input placeholder="Name (optional)" bind:value={name} maxlength="60" />

    <input
      class="hidden"
      type="file"
      multiple
      accept=".csv,.tsv,.txt,.json,.jspf,.xspf,.xml,.m3u,.m3u8,.pls,text/*,application/json"
      bind:this={input}
      onchange={pick}
    />
    <button class="primary" disabled={busy} onclick={() => input?.click()}>
      <Icon name="upload" size={20} /> {busy ? 'Importing...' : 'Choose files'}
    </button>

    <textarea rows="4" placeholder={'Or paste songs, one per line:\nRihanna - S&M\nFlo Rida - I Cry'} bind:value={pasted}
    ></textarea>
    <button disabled={busy || !pasted.trim()} onclick={() => run([{ text: pasted }])}>Import pasted songs</button>

    <details>
      <summary>How to get a file from a streaming service</summary>
      <ul class="small">
        <li>
          <b>Spotify, YouTube Music, Pandora, Tidal, Deezer, Amazon and others:</b>
          <a href="https://www.tunemymusic.com/" target="_blank" rel="noreferrer">Tune My Music</a> can read a playlist
          and export it to a file. Choose your service as the source and <b>Export to file</b> as the destination, then
          choose the file here. Soundiiz works too.
        </li>
        <li><b>Spotify only:</b> <a href="https://exportify.net/" target="_blank" rel="noreferrer">Exportify</a>, or the <code>Playlist1.json</code> in your account data.</li>
        <li>
          <b>YouTube Music:</b> Google Takeout lists only video ids. DJINN asks YouTube for each title, so that one file
          does go online.
        </li>
        <li><b>Apple Music:</b> File &gt; Library &gt; Export Playlist.</li>
        <li><b>Other music players:</b> M3U, PLS and XSPF files work.</li>
      </ul>
    </details>

    {#each results as result (result.id)}
      <a class="result" href="#/playlist/{result.id}">
        <span class="ellipsis">{result.name}</span>
        <span class="small muted">
          {result.found} found{#if result.missing}, <b class="bad">{result.missing} not in your library</b>{/if}
        </span>
      </a>
    {/each}
    {#if skipped}
      <p class="muted small">{skipped} {skipped === 1 ? 'line' : 'lines'} didn't name a song and were left out.</p>
    {/if}
  </div>
{:else}
  <button class="opener" onclick={() => (open = true)}><Icon name="upload" size={18} /> Import a playlist</button>
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
