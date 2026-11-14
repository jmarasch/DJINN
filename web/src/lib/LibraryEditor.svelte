<script lang="ts">
  // Admin > Library editor: the workspace, the compare, and which folders
  // can be changed. The whole editor sits behind the protected areas' lock.
  import { takeEditorJump } from './editorJump.svelte'
  import EditorWorkspace from './EditorWorkspace.svelte'
  import LibraryCompare from './LibraryCompare.svelte'

  /** A folder another page asked to open. */
  const startFolder = takeEditorJump()

  let view = $state<'editor' | 'compare'>('editor')
</script>

<div class="tabs" role="tablist">
  <button role="tab" aria-selected={view === 'editor'} class:active={view === 'editor'} onclick={() => (view = 'editor')}>
    Editor
  </button>
  <button role="tab" aria-selected={view === 'compare'} class:active={view === 'compare'} onclick={() => (view = 'compare')}>
    Compare and folders
  </button>
</div>

{#if view === 'editor'}
  <EditorWorkspace {startFolder} />
{:else}
  <LibraryCompare />
{/if}

<style>
  .tabs {
    display: flex;
    gap: 8px;
    margin-bottom: 12px;
  }

  .tabs button.active {
    border-color: var(--accent);
    box-shadow: 0 0 10px var(--glow);
  }
</style>
