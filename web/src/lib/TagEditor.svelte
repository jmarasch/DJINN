<script lang="ts">
  // Corrects songs' tags. DJINN keeps the corrections itself and never
  // changes the music files. Only changed fields are sent; an emptied one
  // goes back to what the file says.
  import { untrack } from 'svelte'
  import { api } from './api'
  import Sheet from './Sheet.svelte'
  import { toast } from './state.svelte'
  import type { TagChange, TagField } from './types'

  let {
    title,
    trackIds,
    fields,
    initial,
    edited = [],
    onclose,
    onsaved,
  }: {
    title: string
    trackIds: number[]
    fields: TagField[]
    /** What the songs show now, per field. */
    initial: Partial<Record<TagField, string>>
    /** Fields already corrected, which can go back to the file's value. */
    edited?: TagField[]
    onclose: () => void
    onsaved: () => void
  } = $props()

  const labels: Record<TagField, string> = {
    title: 'Title',
    artist: 'Artist',
    album: 'Album',
    album_artist: 'Album artist',
    year: 'Year',
    genre: 'Genre',
  }
  const start = untrack(() => Object.fromEntries(fields.map((f) => [f, initial[f] ?? ''])))
  let values = $state<Record<string, string>>({ ...start })
  let saving = $state(false)

  async function send(change: TagChange) {
    saving = true
    try {
      await api.editTracks(change)
      toast('Saved')
      onsaved()
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not save', true)
    } finally {
      saving = false
    }
  }

  function save(e: SubmitEvent) {
    e.preventDefault()
    const change: TagChange = { track_ids: trackIds }
    for (const f of fields) {
      const value = values[f].trim()
      if (value === start[f].trim()) continue
      if (f === 'year' && value && !/^\d{1,4}$/.test(value)) {
        toast('The year should be a number, like 1997', true)
        return
      }
      change[f] = value === '' ? null : f === 'year' ? Number(value) : value
    }
    if (Object.keys(change).length === 1) {
      onclose()
      return
    }
    void send(change)
  }

  /** Back to the file's value for one field, or all of them. */
  function useFile(only?: TagField) {
    const change: TagChange = { track_ids: trackIds }
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
              Corrected · use the file's
            </button>
          {/if}
        </span>
        <input
          bind:value={values[field]}
          inputmode={field === 'year' ? 'numeric' : undefined}
          maxlength={field === 'year' ? 4 : 300}
        />
      </label>
    {/each}
    <p class="small muted">
      {trackIds.length > 1 ? `Applies to all ${trackIds.length} songs. ` : ''}DJINN keeps corrections
      itself; your music files aren't changed. Empty a field to use what the file says.
    </p>
    <div class="actions">
      <button class="primary" disabled={saving}>Save</button>
      <button type="button" disabled={saving} onclick={() => useFile()}>Use the file's tags</button>
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
