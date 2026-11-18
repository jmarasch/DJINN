<script lang="ts">
  // The library editor in a window of its own, full width. Opening it always
  // starts locked: whatever was unlocked before is closed, and the admin
  // enters their password (and a two-step code at the strictest level) to
  // go in. It locks again when the window closes, after ten minutes without
  // use (then asking again over the work), or from the button here.
  import { t } from '../lib/i18n.svelte'
  import { onMount } from 'svelte'
  import { api } from '../lib/api'
  import EditorWorkspace from '../lib/EditorWorkspace.svelte'
  import LibraryCompare from '../lib/LibraryCompare.svelte'
  import LibraryFolders from '../lib/LibraryFolders.svelte'
  import { route } from '../lib/router.svelte'
  import Tabs from '../lib/Tabs.svelte'
  import UnlockForm from '../lib/UnlockForm.svelte'
  import UnlockHost from '../lib/UnlockHost.svelte'

  let stage = $state<'starting' | 'locked' | 'open'>('starting')
  let view = $state<'editor' | 'folders' | 'compare'>('editor')

  /** A folder or words another page asked the editor to start with. */
  const ask = $derived(new URLSearchParams(route.path.split('?')[1] ?? ''))

  onMount(() => {
    // Every time the window opens, whatever was unlocked is locked again first.
    // (With the protected level off there's nothing to unlock.)
    void api
      .protectedLeave()
      .catch(() => {})
      .then(() => api.protectedStatus())
      .then((status) => (stage = status.open ? 'open' : 'locked'))
      .catch(() => (stage = 'locked'))
    document.title = 'DJINN library editor'
    const leave = () => void fetch('/api/protected/leave', { method: 'POST', keepalive: true }).catch(() => {})
    window.addEventListener('pagehide', leave)
    return () => window.removeEventListener('pagehide', leave)
  })

  async function lockAndClose() {
    await api.protectedLeave().catch(() => {})
    stage = 'locked'
    window.close()
  }
</script>

<div class="window">
  <header>
    <h1>{t('admin.menu.editor')}</h1>
    {#if stage === 'open'}
      <Tabs
        items={[
          ['editor', t('ew.editor')],
          ['folders', t('ew.folders')],
          ['compare', t('ew.compare')],
        ]}
        active={view}
        onselect={(id) => (view = id as 'editor' | 'folders' | 'compare')}
      />
    {/if}
    <span class="spacer"></span>
    {#if stage === 'open'}
      <button onclick={lockAndClose}>{t('ew.lock_close')}</button>
    {/if}
  </header>

  <div class="content">
    {#if stage === 'open'}
      <UnlockHost />
      {#if view === 'editor'}
        {#key route.path}
          <EditorWorkspace windowed startFolder={ask.get('folder') ?? ''} startSearch={ask.get('search') ?? ''} />
        {/key}
      {:else}
        <div class="page">
          {#if view === 'folders'}<LibraryFolders />{:else}<LibraryCompare />{/if}
        </div>
      {/if}
    {:else if stage === 'locked'}
      <div class="gate card">
        <h2>{t('ew.unlock')}</h2>
        <UnlockForm
          skipIfOpen={false}
          onopened={() => (stage = 'open')}
          note={t('ew.note')}
        />
      </div>
    {/if}
  </div>
</div>

<style>
  .window {
    position: fixed;
    inset: 0;
    z-index: 5;
    display: flex;
    flex-direction: column;
    background: var(--page-bg, var(--bg));
  }

  header {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 10px 16px 0;
  }

  h1 {
    margin: 0 0 14px;
    font-size: 1.3rem;
    white-space: nowrap;
  }

  header :global(.subtabs) {
    flex: 0 1 520px;
  }

  .spacer {
    flex: 1;
  }

  header > button {
    margin-bottom: 14px;
  }

  .content {
    flex: 1;
    min-height: 0;
    overflow: auto;
    padding: 0 16px 16px;
  }

  .page {
    max-width: 1100px;
    margin: 0 auto;
  }

  .gate {
    max-width: 460px;
    margin: 12vh auto 0;
  }
</style>
