<script lang="ts">
  // Admin-only: adds an album or artist to the auto pool.
  import { api, ApiError } from './api'
  import Icon from './Icon.svelte'
  import { isAdmin, toast } from './state.svelte'
  import type { PoolKind } from './types'

  let { kind, refId }: { kind: PoolKind; refId: number } = $props()
  let added = $state(false)

  async function add() {
    try {
      await api.addToPool(kind, refId)
      added = true
      toast(`Added to the auto pool`)
    } catch (e) {
      if (e instanceof ApiError && e.status === 409) added = true
      toast(e instanceof Error ? e.message : 'Could not add to the pool', !added)
    }
  }
</script>

{#if isAdmin()}
  <button class="pool" disabled={added} onclick={add}>
    <Icon name={added ? 'check' : 'plus'} size={18} />
    {added ? 'In auto pool' : 'Add to auto pool'}
  </button>
{/if}

<style>
  .pool {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    margin-top: 10px;
    padding: 8px 14px;
    border-radius: 999px;
    background: var(--surface-2);
    color: var(--text);
    font-size: 0.875rem;
  }
</style>
