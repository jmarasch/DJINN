<script lang="ts">
  // Back up and restore (admin): download everything DJINN knows as one
  // file, or put a backup back, here or on another computer with the music
  // in a different place.
  import { api } from './api'
  import { reloadAfterRestart } from './restart'
  import FolderPicker from './FolderPicker.svelte'
  import { toast } from './state.svelte'
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
      toast(e instanceof Error ? e.message : 'Could not read that backup', true)
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
        'Replace everything DJINN knows on this computer with this backup? DJINN restarts, and what is here now is kept as djinn.db.before-restore.',
      )
    )
      return
    try {
      await api.startRestore(backup.folders.map((f, i) => [f.from, targets[i].trim()]))
      restarting = true
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not start the restore', true)
    }
  }

  // Wait for DJINN to go away and come back, then load the page again
  // (signing in with an account from the backup).
  $effect(() => {
    if (restarting) return reloadAfterRestart()
  })
</script>

<section class="card">
  <h2>Back up and restore</h2>
  {#if restarting}
    <p>Restoring the backup. DJINN is restarting; this page reloads when it's back.</p>
    <p class="small muted">Sign in with an account from the backup.</p>
  {:else if backup}
    <p>
      Backup from <b>{new Date(backup.created_ms).toLocaleString()}</b>, made by DJINN {backup.version}.
    </p>
    {#if backup.folders.length}
      <p class="small muted">
        Where is this music on this computer? Leave a folder empty if its music isn't here; those songs
        show as missing.
      </p>
      {#each backup.folders as folder, i (folder.from)}
        <div class="folder">
          <span class="small">Was <code>{folder.from}</code></span>
          <div class="row">
            <input bind:value={targets[i]} placeholder="Folder on this computer" />
            <button onclick={() => (browsing = browsing === i ? null : i)}>Browse</button>
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
      Restoring replaces everything DJINN knows on this computer with the backup. What's here now is kept as
      <code>djinn.db.before-restore</code> next to the database.
    </p>
    <div class="actions">
      <button class="primary" onclick={restore}>Restore and restart</button>
      <button onclick={cancel}>Cancel</button>
    </div>
  {:else}
    <p class="small muted">
      One file with everything DJINN knows: settings, accounts, playlists, history, party styles, speakers and
      casts, tag corrections and artwork. Not your music or the AI models. Keep it private, since it holds your
      accounts.
    </p>
    <div class="actions">
      <a class="button" href="/api/backup" download>Download a backup</a>
      <label class="button" class:disabled={uploading}>
        {uploading ? 'Reading the backup…' : 'Restore from a backup…'}
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
