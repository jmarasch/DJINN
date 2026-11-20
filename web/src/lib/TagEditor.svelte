<script lang="ts">
  // Corrects songs' tags. DJINN keeps the corrections itself and never
  // changes the music files. Only changed fields are sent; an emptied one
  // goes back to what the file says.
  import { t } from './i18n.svelte'
  import { untrack } from 'svelte'
  import { api } from './api'
  import Sheet from './Sheet.svelte'
  import { toast } from './state.svelte'
  import type { TagField, TagFields } from './types'

  let {
    title,
    trackIds = [],
    save: store,
    applies,
    fields,
    initial,
    edited = [],
    onclose,
    onsaved,
  }: {
    title: string
    trackIds?: number[]
    /** Saves the changed fields some other way than on `trackIds`. */
    save?: (change: TagFields) => Promise<void>
    /** Which songs the change applies to, when not just `trackIds`. */
    applies?: string
    fields: TagField[]
    /** What the songs show now, per field. */
    initial: Partial<Record<TagField, string>>
    /** Fields already corrected, which can go back to the file's value. */
    edited?: TagField[]
    onclose: () => void
    onsaved: () => void
  } = $props()

  const labels: Record<TagField, string> = $derived({
    title: t('ed.title'),
    artist: t('ed.artist'),
    album: t('ed.album'),
    album_artist: t('ed.album_artist'),
    year: t('ed.year'),
    genre: t('te.genres'),
  })
  const start = untrack(() => Object.fromEntries(fields.map((f) => [f, initial[f] ?? ''])))
  let values = $state<Record<string, string>>({ ...start })
  let saving = $state(false)

  async function send(change: TagFields) {
    saving = true
    try {
      await (store ? store(change) : api.editTracks({ track_ids: trackIds, ...change }))
      toast(t('common.saved'))
      onsaved()
    } catch (e) {
      toast(e instanceof Error ? e.message : t('common.save_failed_short'), true)
    } finally {
      saving = false
    }
  }

  function save(e: SubmitEvent) {
    e.preventDefault()
    const change: TagFields = {}
    for (const f of fields) {
      const value = values[f].trim()
      if (value === start[f].trim()) continue
      if (f === 'year' && value && !/^\d{1,4}$/.test(value)) {
        toast(t('te.year_number'), true)
        return
      }
      change[f] = value === '' ? null : f === 'year' ? Number(value) : value
    }
    if (!Object.keys(change).length) {
      onclose()
      return
    }
    void send(change)
  }

  /** Back to the file's value for one field, or all of them. */
  function useFile(only?: TagField) {
    const change: TagFields = {}
    for (const f of only ? [only] : fields) change[f] = null
    void send(change)
  }
</script>

<Sheet {title} {onclose}>
  <form onsubmit={save}>
    {#each fields as field (field)}
      <label>
        <span class="label">
          {labels[field]}
          {#if edited.includes(field)}
            <button type="button" class="link" disabled={saving} onclick={() => useFile(field)}>
              {t('te.corrected')}
            </button>
          {/if}
        </span>
        <input
          bind:value={values[field]}
          inputmode={field === 'year' ? 'numeric' : undefined}
          placeholder={field === 'genre' ? t('te.genre_hint') : undefined}
          maxlength={field === 'year' ? 4 : 300}
        />
      </label>
    {/each}
    <p class="small muted">
      {applies ?? (trackIds.length > 1 ? t('te.applies_all', { n: trackIds.length }) : '')}
      {t('te.note')}
    </p>
    <div class="actions">
      <button class="primary" disabled={saving}>{t('common.save')}</button>
      <button type="button" disabled={saving} onclick={() => useFile()}>{t('te.use_file')}</button>
    </div>
  </form>
</Sheet>

<style>
  form {
    display: grid;
    gap: 12px;
    padding: 4px 0 8px;
  }

  label {
    display: grid;
    gap: 4px;
  }

  .label {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: 8px;
    font-size: 0.875rem;
  }

  .link {
    padding: 0;
    min-height: 0;
    border: 0;
    background: none;
    color: var(--accent);
    font-size: 0.8125rem;
  }

  p {
    margin: 0;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
</style>
