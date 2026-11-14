<script lang="ts">
  // Admin > Library editor > Library folders: what DJINN may do in each
  // library folder, and the editor's trash. Read only folders can always be
  // looked at (the compare and the Problems page read every folder) but
  // nothing in them changes. Read and write lets the editor change tags,
  // covers, names and places, copy in and delete; every change is in
  // place, in the history, and can be undone until the trash is emptied.
  import { onMount } from 'svelte'
  import { ApiError, api } from './api'
  import { askToUnlock, unlocked } from './protected.svelte'
  import { toast } from './state.svelte'
  import type { EditableFolder } from './types'

  let folders = $state<EditableFolder[]>([])
  let trash = $state<{ files: number; bytes: number } | null>(null)
  let locked = $state(false)
  let busy = $state(false)

  async function load() {
    try {
      folders = await api.editableFolders()
      trash = await api.editorTrash()
      locked = false
    } catch (e) {
      if (e instanceof ApiError && e.status === 423) locked = true
      else toast(e instanceof Error ? e.message : 'Could not load', true)
    }
  }

  onMount(() => void load())

  async function setWrite(f: EditableFolder, on: boolean) {
    try {
      await unlocked(() => api.setFolderEditable(f.path, on))
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not change it', true)
    }
    await load()
  }

  async function emptyTrash() {
    if (!trash?.files) return
    if (!confirm(`Delete the ${trash.files} files in the trash for good? Those deletes can't be undone after this.`)) return
    busy = true
    try {
      const gone = await unlocked(() => api.editorEmptyTrash())
      toast(`${gone.files.toLocaleString()} files deleted for good`)
      await load()
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not empty it', true)
    } finally {
      busy = false
    }
  }

  const mb = (bytes: number) => `${(bytes / 1_000_000).toLocaleString(undefined, { maximumFractionDigits: 1 })} MB`
</script>

<section class="card">
  <h3>Library folders</h3>
  {#if locked}
    <p class="small">The editor is locked. <button class="link" onclick={async () => (await askToUnlock()) && load()}>Unlock</button></p>
  {:else}
    <p class="small muted">
      Each library folder is <b>read only</b> until you make it <b>read and write</b>. In a read only folder you can see
      everything (the editor, the compare and the Problems page read every folder), but nothing changes, so use it for a
      folder someone else looks after. In a read and write folder the editor changes the files themselves, in place:
      tags, covers, names and places, copying in and deleting. Every change is in the editor's history and can be undone.
      Making a folder read and write checks that DJINN can write there.
    </p>
    <ul class="folders">
      {#each folders as f (f.path)}
        <li>
          <div>
            <strong>{f.name}</strong>
            <span class="badge" class:on={f.editable}>{f.editable ? 'Read and write' : 'Read only'}</span>
            <div class="small mono muted path">{f.path}</div>
          </div>
          <button class="small-button" onclick={() => setWrite(f, !f.editable)}>
            {f.editable ? 'Make read only' : 'Make read and write'}
          </button>
        </li>
      {/each}
    </ul>

    <h4>Trash</h4>
    <p class="small muted">
      Deleted songs wait in a hidden <code>.djinn-trash</code> folder inside their library folder, keeping their place, so a
      delete can be undone from the editor's history. Library checks skip it. Emptying it deletes them for good.
    </p>
    {#if trash}
      <p class="small">
        {trash.files ? `${trash.files.toLocaleString()} files, ${mb(trash.bytes)}` : 'Empty.'}
        {#if trash.files}
          <button class="small-button danger" disabled={busy} onclick={emptyTrash}>Empty the trash</button>
        {/if}
      </p>
    {/if}
  {/if}
</section>

<style>
  .folders {
    list-style: none;
    margin: 8px 0;
    padding: 0;
  }

  .folders li {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 12px;
    padding: 8px 0;
    border-top: 1px solid var(--line);
  }

  .badge.on {
    background: var(--accent);
    color: #000;
  }

  .path {
    overflow-wrap: anywhere;
  }
</style>
