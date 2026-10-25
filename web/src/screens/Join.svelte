<script lang="ts">
  // Shown on the host's screen (or a TV) so guests can scan in.
  import JoinCard from '../lib/JoinCard.svelte'
</script>

<div class="page">
  <h1>Join the party</h1>
  <JoinCard big />
</div>

<style>
  .page {
    min-height: 100dvh;
    max-width: 640px;
    margin: 0 auto;
    padding: 32px 16px;
    display: flex;
    flex-direction: column;
    align-items: center;
  }

  h1 {
    font-size: clamp(2rem, 6vw, 3rem);
    color: var(--accent);
    margin: 0 0 24px;
  }
</style>
