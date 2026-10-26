<script lang="ts">
  // Admin: queue every song on an album or by an artist, in order.
  import { api } from './api'
  import Icon from './Icon.svelte'
  import { isAdmin, toast } from './state.svelte'
  import type { EntryKind } from './types'

  let { kind, refId }: { kind: EntryKind; refId: number } = $props()
  let busy = $state(false)

  async function queueAll() {
    busy = true
    try {
      const { added } = await api.queueAll(kind, refId)
      toast(added ? `Queued ${added} ${added === 1 ? 'song' : 'songs'}` : 'Those songs are already in the queue')
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not queue them', true)
    } finally {
      busy = false
    }
  }
</script>

{#if isAdmin()}
  <button class="all" disabled={busy} onclick={queueAll}><Icon name="queue" size={18} /> Queue all</button>
{/if}

<style>
  .all {
    min-height: 36px;
    margin-top: 10px;
    padding: 0 14px;
    border-radius: 999px;
    font-size: 0.875rem;
  }
</style>
