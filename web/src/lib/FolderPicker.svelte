<script lang="ts">
  // Browse the server's folders and pick one (admin).
  import { onMount } from 'svelte'
  import { api } from './api'
  import Icon from './Icon.svelte'
  import type { FolderListing } from './types'

  let { onpick }: { onpick: (path: string) => void } = $props()
  let listing = $state<FolderListing | null>(null)
  let error = $state('')

  async function open(path?: string) {
    error = ''
    try {
      listing = await api.browseFolders(path)
    } catch (e) {
      error = e instanceof Error ? e.message : 'Could not open that folder'
    }
  }

  onMount(() => open())
</script>

{#if listing}
  <div class="places">
    {#each listing.places as place (place.path)}
      <button class="chip" onclick={() => open(place.path)}>{place.name}</button>
    {/each}
  </div>
  <div class="current">
    <Icon name="folder" size={18} />
    <span class="ellipsis">{listing.path ?? 'This computer'}</span>
  </div>
  {#if error}<p class="error">{error}</p>{/if}
  <div class="dirs">
    {#if listing.parent !== null}
      <!-- An empty parent is the top level (the drives on Windows). -->
      <button class="dir" onclick={() => open(listing?.parent ?? undefined)}>
        <Icon name="back" size={18} /> Up
      </button>
    {/if}
    {#each listing.dirs as dir (dir.path)}
      <button class="dir" onclick={() => open(dir.path)}>
        <Icon name="folder" size={18} />
        <span class="ellipsis">{dir.name}</span>
      </button>
    {:else}
      <p class="muted small">No folders inside.</p>
    {/each}
  </div>
  {#if listing.path}
    <button class="primary use" onclick={() => listing?.path && onpick(listing.path)}>
      Use this folder
    </button>
  {/if}
{:else if error}
  <p class="error">{error}</p>
{:else}
  <p class="muted">Loading folders...</p>
{/if}

<style>
  .places {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin-bottom: 10px;
  }

  .chip {
    min-height: 34px;
    padding: 0 12px;
    border-radius: 999px;
    font-size: 0.875rem;
  }

  .current {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 0;
    font-weight: 600;
    min-width: 0;
  }

  .dirs {
    display: flex;
    flex-direction: column;
    max-height: 45dvh;
    overflow-y: auto;
    border-top: 1px solid var(--line);
  }

  .dir {
    justify-content: flex-start;
    gap: 10px;
    min-height: 46px;
    padding: 0 4px;
    border-radius: 0;
    border-bottom: 1px solid var(--line);
    background: transparent;
    text-align: left;
    min-width: 0;
  }

  .use {
    width: 100%;
    margin-top: 12px;
  }
</style>
