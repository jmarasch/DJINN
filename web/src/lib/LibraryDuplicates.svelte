<script lang="ts">
  // Admin > Library > Possible duplicates: copies of a song, found by name,
  // by identified recording, or by sound, with the copy to keep first.
  // DJINN never deletes anything; the admin tidies the folders.
  import { api } from './api'
  import { app, toast } from './state.svelte'
  import type { DuplicateGroup, DuplicateListing, DuplicateReason } from './types'

  const why: Record<DuplicateReason, string> = {
    name: 'same title and artist',
    recording: 'identified as the same recording',
    sound: 'sound the same',
  }
  const reasons = (group: DuplicateGroup) => {
    const text = group.reasons.map((r) => why[r]).join(', ')
    return text.charAt(0).toUpperCase() + text.slice(1)
  }
  const formats: Record<string, string> = { mpeg: 'MP3', mp4: 'M4A', vorbis: 'Ogg Vorbis', wav: 'WAV', flac: 'FLAC', aac: 'AAC' }

  let listing = $state<DuplicateListing | null>(null)
  let groups = $state<DuplicateGroup[]>([])
  let open = $state(false)
  let loading = $state(false)

  async function load() {
    loading = true
    try {
      listing = await api.duplicates(0)
      groups = listing.groups
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not look for duplicates', true)
    } finally {
      loading = false
    }
  }

  $effect(() => {
    load()
  })

  // A finished library check may have found new copies.
  let lastDone: boolean | undefined
  $effect(() => {
    const done = app.scan?.done
    if (done && lastDone === false) load()
    lastDone = done
  })

  async function more() {
    loading = true
    try {
      const next = await api.duplicates(groups.length)
      listing = next
      groups = [...groups, ...next.groups]
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not load more', true)
    } finally {
      loading = false
    }
  }

  async function notCopies(group: DuplicateGroup) {
    try {
      await api.notDuplicates(group.songs.map((s) => s.id))
      groups = groups.filter((g) => g !== group)
      if (listing) listing = { ...listing, total: listing.total - 1 }
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not save that', true)
    }
  }

  const n = (x: number) => x.toLocaleString()
  const length = (ms: number | null) => {
    if (ms === null) return ''
    const s = Math.round(ms / 1000)
    return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`
  }
  const facts = (c: DuplicateGroup['songs'][number]) =>
    [
      c.format ? (formats[c.format] ?? c.format.toUpperCase()) : null,
      c.kbps ? `${n(c.kbps)} kbps` : null,
      c.sample_rate ? `${(c.sample_rate / 1000).toLocaleString()} kHz` : null,
      length(c.duration_ms),
    ]
      .filter(Boolean)
      .join(', ')
</script>

<section class="card">
  <h2>Possible duplicates</h2>
  {#if !listing}
    <p class="small muted">{loading ? 'Looking...' : ''}</p>
  {:else}
    <p class="small muted">
      {#if listing.total}
        {n(listing.total)} {listing.total === 1 ? 'song has' : 'songs have'} more than one copy ({n(listing.songs)} files).
      {:else}
        No copies found.
      {/if}
      DJINN never deletes files: remove the copies you don't want from your music folders, then check for changes.
      {#if !listing.sound_analysis}
        Turn on sound analysis in Library extras to also find copies that are named differently.
      {/if}
    </p>
    {#if listing.total}
      <button class="small-button" onclick={() => (open = !open)}>{open ? 'Hide the list' : 'Show the list'}</button>
    {/if}
    {#if open}
      {#each groups as group (group.songs.map((s) => s.id).join('-'))}
        <div class="group">
          <div class="small muted">{reasons(group)}</div>
          <ul>
            {#each group.songs as copy (copy.id)}
              <li>
                <div>
                  {#if copy.keep}<span class="keep">Keep</span>{/if}
                  {#if copy.album_id !== null}
                    <a href="#/album/{copy.album_id}">{copy.title}</a>
                  {:else}
                    {copy.title}
                  {/if}
                  {#if copy.artist}<span class="muted"> · {copy.artist}</span>{/if}
                  {#if copy.album}<span class="muted"> · {copy.album}</span>{/if}
                </div>
                <div class="small muted">{facts(copy)}</div>
                <div class="small mono path">{copy.path}</div>
              </li>
            {/each}
          </ul>
          <button class="small-button" onclick={() => notCopies(group)}>Not duplicates</button>
        </div>
      {/each}
      {#if groups.length < listing.total}
        <button class="small-button" disabled={loading} onclick={more}>Show more</button>
      {/if}
    {/if}
  {/if}
</section>

<style>
  .group {
    padding: 10px 0;
    border-top: 1px solid var(--line);
  }

  ul {
    margin: 6px 0 8px;
    padding: 0;
    list-style: none;
  }

  li {
    padding: 6px 0;
    overflow-wrap: anywhere;
  }

  .keep {
    margin-right: 6px;
    padding: 1px 8px;
    border-radius: 999px;
    background: var(--ok);
    color: #fff;
    font-size: 0.75rem;
  }

  .path {
    color: var(--muted);
  }
</style>
