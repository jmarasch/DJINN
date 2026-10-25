<script lang="ts">
  // Second step of first-run setup: where the music lives.
  import { onMount } from 'svelte'
  import { api } from '../lib/api'
  import FolderPicker from '../lib/FolderPicker.svelte'
  import Icon from '../lib/Icon.svelte'
  import { app, toast } from '../lib/state.svelte'
  import type { LibraryRoot } from '../lib/types'

  let roots = $state<LibraryRoot[]>([])
  let picking = $state(false)

  onMount(async () => {
    roots = await api.libraryRoots()
    picking = roots.length === 0
  })

  async function add(path: string) {
    try {
      roots = await api.addLibraryRoot(path)
      picking = false
      toast('Added. Scanning it now.')
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not add that folder', true)
    }
  }

  function done() {
    app.setupLibrary = false
    history.replaceState(null, '', '/')
  }
</script>

<div class="card">
  <h1>Add your music</h1>
  <p class="muted">Pick the folders where your music lives. You can add more later under Admin.</p>
  {#each roots as root (root.path)}
    <div class="root"><Icon name="folder" size={18} /> <span class="path">{root.path}</span></div>
  {/each}
  {#if picking}
    <FolderPicker onpick={add} />
  {:else}
    <button onclick={() => (picking = true)}>Add another folder</button>
  {/if}
  <button class:primary={roots.length > 0} onclick={done}>{roots.length ? 'Done' : 'Skip for now'}</button>
</div>

<style>
  .root {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .path {
    overflow-wrap: anywhere;
  }
</style>
