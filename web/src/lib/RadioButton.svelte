<script lang="ts">
  // Admin: start a radio from a song, an artist, or a genre.
  import { api } from './api'
  import Icon from './Icon.svelte'
  import { isAdmin, toast } from './state.svelte'
  import type { RadioSeed } from './types'

  let { seed, label = 'Radio', onstart }: { seed: RadioSeed; label?: string; onstart?: () => void } = $props()
  let busy = $state(false)

  async function start() {
    busy = true
    try {
      await api.playRadio(seed)
      toast('Radio on: songs like this play when the requests run out')
      onstart?.()
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not start the radio', true)
    } finally {
      busy = false
    }
  }
</script>

{#if isAdmin()}
  <button class="radio" disabled={busy} onclick={start}><Icon name="radio" size={18} /> {label}</button>
{/if}

<style>
  .radio {
    min-height: 36px;
    margin-top: 10px;
    padding: 0 14px;
    gap: 6px;
    border-radius: 999px;
    font-size: 0.875rem;
  }
</style>
