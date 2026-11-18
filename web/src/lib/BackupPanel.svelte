<script lang="ts">
  // Back up and restore (admin): download everything DJINN knows as one
  // file, or put a backup back, here or on another computer with the music
  // in a different place.
  import { api } from './api'
  import { reloadAfterRestart } from './restart'
  import FolderPicker from './FolderPicker.svelte'
  import { toast } from './state.svelte'
  import { locale, t } from './i18n.svelte'
  import type { UploadedBackup } from './types'

  let uploading = $state(false)
  let backup = $state<UploadedBackup | null>(null)
  /** Where each of the backup's music folders is on this computer. */
  let targets = $state<string[]>([])
  let browsing = $state<number | null>(null)
  let restarting = $state(false)

  async function upload(e: Event) {
    const input = e.currentTarget as HTMLInputElement
    const file = input.files?.[0]
    input.value = ''
    if (!file) return
    uploading = true
    try {
      backup = await api.uploadBackup(file)
      targets = backup.folders.map((f) => (f.here ? f.from : ''))
    } catch (e) {
      toast(e instanceof Error ? e.message : t('bk.read_failed'), true)
    } finally {
      uploading = false
    }
  }

  async function cancel() {
    backup = null
    browsing = null
    await api.cancelRestore().catch(() => {})
  }

  async function restore() {
    if (!backup) return
    if (
      !confirm(
        t('bk.restore_q'),
      )
    )
      return
    try {
      await api.startRestore(backup.folders.map((f, i) => [f.from, targets[i].trim()]))
      restarting = true
    } catch (e) {
      toast(e instanceof Error ? e.message : t('bk.restore_failed'), true)
    }
  }

  // Wait for DJINN to go away and come back, then load the page again
  // (signing in with an account from the backup).
  $effect(() => {
    if (restarting) return reloadAfterRestart()
  })
</script>

<section class="card">
  <h2>{t('bk.title')}</h2>
  {#if restarting}
    <p>{t('bk.restoring')}</p>
    <p class="small muted">{t('bk.sign_in')}</p>
  {:else if backup}
    <p>
      {t('bk.from', { date: new Date(backup.created_ms).toLocaleString(locale.code), version: backup.version })}
    </p>
    {#if backup.folders.length}
      <p class="small muted">
        {t('bk.where')}
      </p>
      {#each backup.folders as folder, i (folder.from)}
        <div class="folder">
          <span class="small">{t('bk.was')} <code>{folder.from}</code></span>
          <div class="row">
            <input bind:value={targets[i]} placeholder={t('bk.folder_here')} />
            <button onclick={() => (browsing = browsing === i ? null : i)}>{t('bk.browse')}</button>
          </div>
          {#if browsing === i}
            <FolderPicker
              onpick={(path) => {
                targets[i] = path
                browsing = null
              }}
            />
          {/if}
        </div>
      {/each}
    {/if}
    <p class="small muted">
      {t('bk.replaces')}
      <code>djinn.db.before-restore</code> {t('bk.next_to')}
    </p>
    <div class="actions">
      <button class="primary" onclick={restore}>{t('bk.restore')}</button>
      <button onclick={cancel}>{t('common.cancel')}</button>
    </div>
  {:else}
    <p class="small muted">
      {t('bk.about')}
    </p>
    <div class="actions">
      <a class="button" href="/api/backup" download>{t('bk.download')}</a>
      <label class="button" class:disabled={uploading}>
        {uploading ? t('bk.reading') : t('bk.restore_from')}
        <input type="file" accept=".zip,application/zip" hidden disabled={uploading} onchange={upload} />
      </label>
    </div>
  {/if}
</section>

<style>
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }

  label.button {
    cursor: pointer;
  }

  label.disabled {
    opacity: 0.6;
    pointer-events: none;
  }

  .folder {
    display: grid;
    gap: 6px;
    margin-bottom: 12px;
  }

  .row {
    display: flex;
    gap: 8px;
  }

  .row input {
    flex: 1;
    min-width: 0;
  }

  code {
    word-break: break-all;
  }
</style>
