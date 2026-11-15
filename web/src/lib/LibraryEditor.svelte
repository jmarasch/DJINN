<script lang="ts">
  // Admin > Library editor: the workspace, the library folders (read only
  // or read and write, and the trash), and the compare. The whole editor sits behind the protected areas' lock.
  import { takeEditorJump } from './editorJump.svelte'
  import EditorWorkspace from './EditorWorkspace.svelte'
  import LibraryCompare from './LibraryCompare.svelte'
  import LibraryFolders from './LibraryFolders.svelte'
  import Tabs from './Tabs.svelte'

  /** A folder or search another page asked for. */
  const start = takeEditorJump()

  let view = $state<'editor' | 'folders' | 'compare'>('editor')
</script>

<Tabs
  items={[
    ['editor', 'Editor'],
    ['folders', 'Library folders'],
    ['compare', 'Compare'],
  ]}
  active={view}
  onselect={(id) => (view = id as 'editor' | 'folders' | 'compare')}
/>

{#if view === 'editor'}
  <EditorWorkspace startFolder={start.folder} startSearch={start.search} />
{:else if view === 'folders'}
  <LibraryFolders />
{:else}
  <LibraryCompare />
{/if}
