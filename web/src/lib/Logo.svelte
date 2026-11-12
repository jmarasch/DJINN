<script lang="ts">
  // DJINN's mark: a record whose grooves open into a waveform. The rings
  // take the accent color and the bars the second color, so it follows the
  // light and dark themes.
  let { size = 40 }: { size?: number } = $props()
  const id = `logo-${Math.random().toString(36).slice(2, 8)}`
  const bars = 'M34.5 17V47M40 9V55M45.5 16V48M51 21V43M56.5 26V38'
</script>

<svg width={size} height={size} viewBox="0 0 64 64" aria-hidden="true" class="logo">
  <mask {id}>
    <rect x="-8" y="-8" width="80" height="80" fill="#fff" />
    <path d={bars} stroke="#000" stroke-width="7" stroke-linecap="round" />
  </mask>
  <g mask="url(#{id})">
    <path
      d="M30.16 22.29A11 11 0 1 0 30.16 41.71M34.85 19.39A16 16 0 1 0 34.85 44.61M40.61 17.95A21 21 0 1 0 40.61 46.05M47.05 18.22A26 26 0 1 0 47.05 45.78"
      fill="none"
      stroke="var(--accent)"
      stroke-width="3.4"
      stroke-linecap="round"
    />
    <circle cx="25" cy="32" r="4.6" fill="var(--accent)" />
  </g>
  <path d={bars} fill="none" stroke="var(--accent-2)" stroke-width="4" stroke-linecap="round" />
</svg>

<style>
  .logo {
    flex: none;
    filter: drop-shadow(0 0 6px rgb(180 140 255 / 0.35));
  }
</style>
