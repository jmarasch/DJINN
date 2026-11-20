<script lang="ts">
  // Admin > Library > Possible duplicates: copies of a song, found by name,
  // by identified recording, or by sound, with the copy to keep first.
  // DJINN never deletes anything; the admin tidies the folders.
  import { api } from './api'
  import { app, toast } from './state.svelte'
  import { locale, t } from './i18n.svelte'
  import type { DuplicateGroup, DuplicateListing, DuplicateReason } from './types'

  const reasons = (group: DuplicateGroup) => {
    const text = group.reasons.map((r: DuplicateReason) => t(`dup.${r}`)).join(', ')
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
      toast(e instanceof Error ? e.message : t('dup.look_failed'), true)
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
      toast(e instanceof Error ? e.message : t('common.load_more_failed'), true)
    } finally {
      loading = false
    }
  }

  async function setAcross(on: boolean) {
    try {
      await api.setDuplicatesAcrossAlbums(on)
      await load()
    } catch (e) {
      toast(e instanceof Error ? e.message : t('sc.save_failed'), true)
    }
  }

  async function notCopies(group: DuplicateGroup) {
    try {
      await api.notDuplicates(group.songs.map((s) => s.id))
      groups = groups.filter((g) => g !== group)
      if (listing) listing = { ...listing, total: listing.total - 1 }
    } catch (e) {
      toast(e instanceof Error ? e.message : t('sc.save_failed'), true)
    }
  }

  const n = (x: number) => x.toLocaleString(locale.code)
  const length = (ms: number | null) => {
    if (ms === null) return ''
    const s = Math.round(ms / 1000)
    return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`
  }
  const facts = (c: DuplicateGroup['songs'][number]) =>
    [
      c.format ? (formats[c.format] ?? c.format.toUpperCase()) : null,
      c.kbps ? `${n(c.kbps)} kbps` : null,
      c.sample_rate ? `${(c.sample_rate / 1000).toLocaleString(locale.code)} kHz` : null,
      length(c.duration_ms),
    ]
      .filter(Boolean)
      .join(', ')
</script>

<section class="card">
  <h2>{t('dup.title')}</h2>
  {#if !listing}
    <p class="small muted">{loading ? t('dup.looking') : ''}</p>
  {:else}
    <p class="small muted">
      {#if listing.total}
        {t('dup.have', { n: listing.total, files: n(listing.songs) })}
      {:else}
        {t('dup.none')}
      {/if}
      {t('dup.never')}
      {#if !listing.sound_analysis}
        {t('dup.sound_off')}
      {/if}
    </p>
    <label class="switch">
      <input type="checkbox" checked={listing.across_albums} onchange={(e) => setAcross(e.currentTarget.checked)} />
      {t('dup.across')}
    </label>
    <p class="small muted">
      {t('dup.across_note')}
    </p>
    {#if listing.total}
      <button class="small-button" onclick={() => (open = !open)}>{open ? t('dup.hide') : t('dup.show')}</button>
    {/if}
    {#if open}
      {#each groups as group (group.songs.map((s) => s.id).join('-'))}
        <div class="group">
          <div class="small muted">{reasons(group)}</div>
          <ul>
            {#each group.songs as copy (copy.id)}
              <li>
                <div>
                  {#if copy.keep}<span class="keep">{t('dup.keep')}</span>{/if}
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
          <button class="small-button" onclick={() => notCopies(group)}>{t('dup.not')}</button>
        </div>
      {/each}
      {#if groups.length < listing.total}
        <button class="small-button" disabled={loading} onclick={more}>{t('common.show_more')}</button>
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
