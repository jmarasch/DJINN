<script lang="ts">
  // The library editor's import work in a window while it runs: bringing
  // songs in, and approving one import or several. A bar when the count is
  // known, a spinner either way. Hiding it leaves the work running.
  import FloatingWindow from './FloatingWindow.svelte'
  import { locale, t } from './i18n.svelte'

  let {
    title,
    line,
    done = null,
    of = null,
    onhide,
  }: {
    title: string
    line: string
    /** How far along, when it's counted. */
    done?: number | null
    of?: number | null
    onhide: () => void
  } = $props()

  const counted = $derived(done !== null && of !== null && of > 0)
</script>

<FloatingWindow {title} onclose={onhide}>
  <p><span class="spinner"></span>{line}</p>
  {#if counted}
    <progress max={of} value={done}></progress>
    <p class="small muted">{t('ip.count', { done: (done ?? 0).toLocaleString(locale.code), of: (of ?? 0).toLocaleString(locale.code) })}</p>
  {:else}
    <progress></progress>
  {/if}
  <p class="small muted">{t('ip.note')}</p>
  {#snippet footer()}
    <button onclick={onhide}>{t('ip.hide')}</button>
  {/snippet}
</FloatingWindow>

<style>
  progress {
    width: 100%;
    accent-color: var(--accent);
  }
</style>
