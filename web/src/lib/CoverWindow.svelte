<script lang="ts">
  // The library editor's cover window: the album's cover large, the ways to
  // change it (a file, remove, back to the files' own or folder.jpg), and
  // a search of the online sources that shows every cover found side by
  // side. DJINN fetches the pictures, so the page talks to no other site.
  import { onMount } from 'svelte'
  import { api } from './api'
  import Art from './Art.svelte'
  import FloatingWindow from './FloatingWindow.svelte'
  import { unlocked } from './protected.svelte'
  import { toast } from './state.svelte'
  import type { EditorSong, EditTarget, ReleaseHit, ReleaseSource } from './types'

  let {
    song,
    count,
    target,
    canEdit,
    busy,
    staged = false,
    onset,
    onremove,
    onreset,
    onfolderart,
    onclose,
  }: {
    /** The chosen song; its album's cover is shown. */
    song: EditorSong
    /** How many songs a change goes to. */
    count: number
    target: EditTarget
    canEdit: boolean
    busy: boolean
    /** Files waiting in an import: no folder.jpg (they aren't in a folder of their own yet). */
    staged?: boolean
    onset: (file: File) => Promise<void>
    onremove: () => Promise<void>
    onreset: () => Promise<void>
    onfolderart: () => Promise<void>
    onclose: () => void
  } = $props()

  const SOURCES: { key: ReleaseSource; label: string }[] = [
    { key: 'musicbrainz', label: 'MusicBrainz' },
    { key: 'deezer', label: 'Deezer' },
    { key: 'itunes', label: 'Apple Music' },
    { key: 'spotify', label: 'Spotify' },
  ]

  // svelte-ignore state_referenced_locally
  let artist = $state(song.album_artist || song.artist || '')
  // svelte-ignore state_referenced_locally
  let album = $state(song.album || '')
  let use = $state<Record<ReleaseSource, boolean>>({ musicbrainz: true, deezer: true, itunes: true, spotify: false })
  let spotifyReady = $state(false)
  let hits = $state<ReleaseHit[]>([])
  /** Covers that wouldn't load (the release has none). */
  let missing = $state<Record<string, boolean>>({})
  let searching = $state(false)
  let searched = $state(false)
  let notes = $state<string[]>([])
  let picked = $state<ReleaseHit | null>(null)
  let input = $state<HTMLInputElement | null>(null)

  const key = (h: ReleaseHit) => `${h.source}:${h.id}`
  const shownHits = $derived(hits.filter((h) => !missing[key(h)]))
  const sourceName = (s: ReleaseSource) => SOURCES.find((x) => x.key === s)?.label ?? s

  onMount(async () => {
    try {
      const account = await api.spotify()
      spotifyReady = !!account.client_id && account.has_secret
      use.spotify = spotifyReady
    } catch {
      spotifyReady = false
    }
  })

  async function search(e: SubmitEvent) {
    e.preventDefault()
    if (!artist.trim() && !album.trim()) return
    searching = true
    picked = null
    missing = {}
    const sources = SOURCES.filter((s) => use[s.key] && (s.key !== 'spotify' || spotifyReady)).map((s) => s.key)
    const found = await Promise.allSettled(
      sources.map((s) => unlocked(() => api.editorReleaseSearch(s, artist.trim(), album.trim()))),
    )
    const next: ReleaseHit[] = []
    const problems: string[] = []
    found.forEach((r, i) => {
      if (r.status === 'fulfilled') next.push(...r.value.slice(0, 12))
      else problems.push(`${sourceName(sources[i])}: ${r.reason instanceof Error ? r.reason.message : 'failed'}`)
    })
    // Like for like: each source's best first, taking turns.
    hits = next
      .map((h, i) => ({ h, rank: next.filter((x, j) => j < i && x.source === h.source).length }))
      .sort((a, b) => a.rank - b.rank)
      .map((x) => x.h)
    notes = problems
    searching = false
    searched = true
  }

  async function useCover() {
    if (!picked) return
    try {
      const res = await fetch(api.editorReleaseCover(picked.source, picked.id))
      if (!res.ok) throw new Error("Couldn't get that cover")
      const blob = await res.blob()
      const kind = blob.type === 'image/png' ? 'png' : 'jpg'
      await onset(new File([blob], `cover.${kind}`, { type: blob.type || 'image/jpeg' }))
    } catch (e) {
      toast(e instanceof Error ? e.message : "Couldn't get that cover", true)
    }
  }
</script>

<FloatingWindow
  title="Album cover"
  subtitle="{song.album ?? 'No album'}{count ? ` · changes go to ${count} songs` : ''}{target === 'library'
    ? ', in the files'
    : ", in DJINN's copy"}"
  {onclose}
>
  <div class="now">
    <Art hash={song.art_hash} size={320} round={10} />
    <div class="actions">
      <input
        type="file"
        accept="image/jpeg,image/png"
        hidden
        bind:this={input}
        onchange={(e) => {
          const file = e.currentTarget.files?.[0]
          if (file) void onset(file)
          e.currentTarget.value = ''
        }}
      />
      <button class="small-button" disabled={!canEdit || busy || !count} onclick={() => input?.click()}>
        Replace from a file...
      </button>
      <button class="small-button" disabled={!canEdit || busy || !count} onclick={onremove}>Remove cover</button>
      {#if staged}
        <!-- Nothing else to go back to, and no folder for folder.jpg yet. -->
      {:else if target === 'database'}
        <button class="small-button" disabled={busy || !count} onclick={onreset}>Use the files' cover</button>
      {:else}
        <button
          class="small-button"
          disabled={busy || !count}
          title="Saves the album's cover as folder.jpg in the songs' folders (replacing one that's there; undo from the history)"
          onclick={onfolderart}
        >
          Make folder.jpg
        </button>
      {/if}
    </div>
  </div>

  <h3>Search online</h3>
  <form class="search" onsubmit={search}>
    <input bind:value={artist} placeholder="Artist" aria-label="Artist" />
    <input bind:value={album} placeholder="Album" aria-label="Album" />
    <button class="small-button" type="submit" disabled={searching}>{searching ? 'Searching...' : 'Search'}</button>
  </form>
  <div class="sources">
    {#each SOURCES as s (s.key)}
      <label class="check small" title={s.key === 'spotify' && !spotifyReady ? 'Set up its account in Admin > Library first' : undefined}>
        <input type="checkbox" bind:checked={use[s.key]} disabled={s.key === 'spotify' && !spotifyReady} />
        {s.label}
      </label>
    {/each}
  </div>
  <p class="small muted">YouTube Music has no way for other programs to search it, so it isn't one of the sources.</p>
  {#each notes as note (note)}<p class="small bad">{note}</p>{/each}

  {#if searched && !searching && !shownHits.length}
    <p class="small muted">No covers found. Try fewer words, or only the album.</p>
  {/if}
  {#if shownHits.length}
    <div class="hits">
      {#each shownHits as h (key(h))}
        <button class="hit" class:on={picked && key(picked) === key(h)} onclick={() => (picked = h)} title="{h.title} · {h.artist}">
          <img
            src={api.editorReleaseCover(h.source, h.id)}
            alt="{h.title} cover"
            loading="lazy"
            onerror={() => (missing = { ...missing, [key(h)]: true })}
          />
          <span class="small name">{h.title}</span>
          <span class="small muted name">{h.artist}{h.date ? ` · ${h.date.slice(0, 4)}` : ''}</span>
          <span class="small muted">{sourceName(h.source)}{h.format ? ` · ${h.format}` : ''}</span>
        </button>
      {/each}
    </div>
  {/if}

  {#snippet footer()}
    <button onclick={onclose}>Close</button>
    <button class="primary" disabled={!picked || !canEdit || busy || !count} onclick={useCover}>Use this cover</button>
  {/snippet}
</FloatingWindow>

<style>
  .now {
    display: flex;
    gap: 16px;
    flex-wrap: wrap;
    align-items: flex-start;
  }

  .actions {
    display: flex;
    flex-direction: column;
    gap: 8px;
    align-items: flex-start;
  }

  h3 {
    margin: 18px 0 8px;
    font-size: 1rem;
  }

  .search {
    display: flex;
    gap: 6px;
    flex-wrap: wrap;
  }

  .search input {
    flex: 1;
    min-width: 12ch;
  }

  .sources {
    display: flex;
    gap: 14px;
    flex-wrap: wrap;
    margin: 8px 0 4px;
  }

  .check {
    display: inline-flex;
    flex-direction: row;
    align-items: center;
    gap: 6px;
  }

  .check input {
    width: auto;
    margin: 0;
  }

  .hits {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(140px, 1fr));
    gap: 10px;
    margin-top: 10px;
  }

  .hit {
    display: flex;
    flex-direction: column;
    align-items: stretch;
    justify-content: flex-start;
    gap: 2px;
    min-height: 0;
    padding: 6px;
    text-align: left;
    border: 2px solid transparent;
    background: var(--surface-2);
  }

  .hit.on {
    border-color: var(--accent);
    box-shadow: var(--glow);
  }

  .hit img {
    width: 100%;
    aspect-ratio: 1;
    object-fit: cover;
    border-radius: 6px;
    background: var(--surface);
  }

  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .bad {
    color: var(--bad);
  }
</style>
