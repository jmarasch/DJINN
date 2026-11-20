<script lang="ts">
  // Admin: queue every song on an album or by an artist, in order.
  import { t } from './i18n.svelte'
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
      toast(added ? t('qa.queued', { n: added }) : t('qa.already'))
    } catch (e) {
      toast(e instanceof Error ? e.message : t('qa.failed'), true)
    } finally {
      busy = false
    }
  }
</script>

{#if isAdmin()}
  <button class="pill" disabled={busy} onclick={queueAll}><Icon name="queue" size={18} /> {t('qa.button')}</button>
{/if}
