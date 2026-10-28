<script lang="ts">
  // Warns the host when Windows has the network set to Public: its firewall
  // then keeps phones out, and nothing else would say why.
  let show = $state(false)

  fetch('/api/join')
    .then((r) => (r.ok ? r.json() : null))
    .then((join) => (show = !!join?.public_network))
    .catch(() => {})
</script>

{#if show}
  <div class="note" role="alert">
    <strong>Phones can't reach DJINN right now.</strong>
    Windows has this network set to Public, which blocks guests. On this computer, open Settings &gt; Network &amp;
    internet, pick the Wi-Fi or Ethernet connection, and choose Private network.
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
