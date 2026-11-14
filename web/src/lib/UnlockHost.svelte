<script lang="ts">
  // Mounted once in Admin: shows the unlock sheet when a protected action
  // needs it, and closes the protected areas again when Admin is left.
  import { onDestroy } from 'svelte'
  import { api } from './api'
  import { unlockAsk } from './protected.svelte'
  import UnlockSheet from './UnlockSheet.svelte'

  onDestroy(() => {
    void api.protectedLeave().catch(() => {})
  })

  function finish(opened: boolean) {
    unlockAsk.current?.resolve(opened)
    unlockAsk.current = null
  }
</script>

{#if unlockAsk.current}
  <UnlockSheet onopened={() => finish(true)} onclose={() => finish(false)} />
{/if}
