<script lang="ts">
  // The DJINN icon: a record whose grooves open into a waveform. Small
  // logos get the smaller copy.
  let { size = 40 }: { size?: number } = $props()
</script>

<img
  class="logo"
  src={size <= 64 ? '/images/logo-small.webp' : '/images/logo.webp'}
  width={size}
  height={size}
  alt=""
/>

<style>
  .logo {
    flex: none;
    filter: drop-shadow(0 0 6px rgb(180 140 255 / 0.35));
  }
</style>
