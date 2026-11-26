<script lang="ts">
  // Asks for the admin's password (and a two-step code at the strictest
  // level) to open the protected areas.
  import { t } from './i18n.svelte'
  import Sheet from './Sheet.svelte'
  import UnlockForm from './UnlockForm.svelte'

  let { onopened, onclose }: { onopened: () => void; onclose: () => void } = $props()
</script>

<Sheet title={t('us.title')} {onclose} above>
  <UnlockForm
    {onopened}
    note={t('us.note')}
  />
</Sheet>
