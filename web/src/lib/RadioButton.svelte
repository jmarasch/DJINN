<script lang="ts">
  // Admin: start a radio from a song, an artist, or a genre.
  import { t } from './i18n.svelte'
  import { api } from './api'
  import Icon from './Icon.svelte'
  import { isAdmin, toast } from './state.svelte'
  import type { RadioSeed } from './types'

  let { seed, label = t('radio.label'), onstart }: { seed: RadioSeed; label?: string; onstart?: () => void } = $props()
  let busy = $state(false)

  async function start() {
    busy = true
    try {
      await api.playRadio(seed)
      toast(t('radio.on'))
      onstart?.()
    } catch (e) {
      toast(e instanceof Error ? e.message : t('radio.failed'), true)
    } finally {
      busy = false
    }
  }
</script>

{#if isAdmin()}
  <button class="pill" disabled={busy} onclick={start}><Icon name="radio" size={18} /> {label}</button>
{/if}
