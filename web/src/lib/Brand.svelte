<script lang="ts">
  // The logo with DJINN's name and what it is, for sign-in and join screens.
  import { t } from './i18n.svelte'
  import Logo from './Logo.svelte'

  let { name = 'DJINN', size = 56 }: { name?: string; size?: number } = $props()
</script>

<div class="brand">
  <Logo {size} />
  <div>
    <h1>{name}</h1>
    <p>{t('brand.tagline')}</p>
  </div>
</div>

<style>
  .brand {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  h1 {
    margin: 0;
    font-size: 1.875rem;
    letter-spacing: 0.08em;
    line-height: 1.1;
    text-shadow: 0 0 16px rgb(180 140 255 / 0.3);
  }

  p {
    margin: 2px 0 0;
    color: var(--muted);
    font-size: 0.6875rem;
    letter-spacing: 0.16em;
    text-transform: uppercase;
  }
</style>
