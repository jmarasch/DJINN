<script lang="ts">
  // Asks for the admin's password (and a two-step code at the strictest
  // level) to open the protected areas.
  import Sheet from './Sheet.svelte'
  import UnlockForm from './UnlockForm.svelte'

  let { onopened, onclose }: { onopened: () => void; onclose: () => void } = $props()
</script>

<Sheet title="Unlock protected areas" {onclose}>
  <UnlockForm
    {onopened}
    note="This changes your music library. It locks again when you leave Admin, or after ten minutes without use."
  />
</Sheet>
