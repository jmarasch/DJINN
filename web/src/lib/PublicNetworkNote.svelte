<script lang="ts">
  // Warns the host when Windows has the network set to Public: its firewall
  // then keeps phones out, and nothing else would say why.
  import { t } from './i18n.svelte'
  let show = $state(false)

  fetch('/api/join')
    .then((r) => (r.ok ? r.json() : null))
    .then((join) => (show = !!join?.public_network))
    .catch(() => {})
</script>

{#if show}
  <div class="note" role="alert">
    <strong>{t('pn.title')}</strong>
    {t('pn.note')}
  </div>
{/if}

<style>
  .note {
    margin: 0 0 16px;
    padding: 12px 14px;
    border-radius: 12px;
    background: var(--bad-surface);
    color: var(--text);
    text-align: left;
  }
</style>
