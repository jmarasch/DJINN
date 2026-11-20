<script lang="ts">
  // Admin > Library Editor > Library folders: what DJINN may do in each
  // library folder, and the editor's trash. Read only folders can always be
  // looked at (the compare and the Problems page read every folder) but
  // nothing in them changes. Read and write lets the editor change tags,
  // covers, names and places, copy in and delete; every change is in
  // place, in the history, and can be undone until the trash is emptied.
  import { onMount } from 'svelte'
  import { ApiError, api } from './api'
  import { askToUnlock, unlocked } from './protected.svelte'
  import { toast } from './state.svelte'
  import { locale, t } from './i18n.svelte'
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
      else toast(e instanceof Error ? e.message : t('ed.load_failed'), true)
    }
  }

  onMount(() => void load())

  async function setWrite(f: EditableFolder, on: boolean) {
    try {
      await unlocked(() => api.setFolderEditable(f.path, on))
    } catch (e) {
      toast(e instanceof Error ? e.message : t('lf.change_failed'), true)
    }
    await load()
  }

  async function emptyTrash() {
    if (!trash?.files) return
    if (!confirm(t('lf.empty_q', { n: trash.files }))) return
    busy = true
    try {
      const gone = await unlocked(() => api.editorEmptyTrash())
      toast(t('lf.deleted', { n: gone.files.toLocaleString(locale.code) }))
      await load()
    } catch (e) {
      toast(e instanceof Error ? e.message : t('lf.empty_failed'), true)
    } finally {
      busy = false
    }
  }

  const mb = (bytes: number) => `${(bytes / 1_000_000).toLocaleString(locale.code, { maximumFractionDigits: 1 })} MB`
</script>

<section class="card">
  <h3>{t('lf.title')}</h3>
  {#if locked}
    <p class="small">
      {t('ed.locked')} <button class="link" onclick={async () => (await askToUnlock()) && load()}>{t('ed.unlock')}</button>
    </p>
  {:else}
    <p class="small muted">
      {t('lf.intro_1')} <b>{t('ed.ro')}</b> {t('lf.intro_2')} <b>{t('ed.rw')}</b>{t('lf.intro_3')}
    </p>
    <ul class="folders">
      {#each folders as f (f.path)}
        <li>
          <div>
            <strong>{f.name}</strong>
            <span class="badge" class:on={f.editable}>{f.editable ? t('lf.rw') : t('lf.ro')}</span>
            <div class="small mono muted path">{f.path}</div>
          </div>
          <button class="small-button" onclick={() => setWrite(f, !f.editable)}>
            {f.editable ? t('lf.make_ro') : t('lf.make_rw')}
          </button>
        </li>
      {/each}
    </ul>

    <h4>{t('lf.trash')}</h4>
    <p class="small muted">
      {t('lf.trash_1')} <code>.djinn-trash</code> {t('lf.trash_2')}
    </p>
    {#if trash}
      <p class="small">
        {trash.files ? t('lf.files_size', { n: trash.files.toLocaleString(locale.code), size: mb(trash.bytes) }) : t('lf.empty')}
        {#if trash.files}
          <button class="small-button danger" disabled={busy} onclick={emptyTrash}>{t('lf.empty_trash')}</button>
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
