<script lang="ts">
  type Name =
    | 'play'
    | 'pause'
    | 'skip'
    | 'plus'
    | 'check'
    | 'close'
    | 'up'
    | 'down'
    | 'search'
    | 'queue'
    | 'disc'
    | 'settings'
    | 'back'
    | 'clock'

  let { name, size = 22 }: { name: Name; size?: number } = $props()

  const paths: Record<Name, string> = {
    play: 'M8 5v14l11-7z',
    pause: 'M7 5h4v14H7zM13 5h4v14h-4z',
    skip: 'M6 5l9 7-9 7zM16 5h2v14h-2z',
    plus: 'M11 5h2v6h6v2h-6v6h-2v-6H5v-2h6z',
    check: 'M9.5 16.2L5.3 12l-1.4 1.4 5.6 5.6L20.1 8.4 18.7 7z',
    close: 'M6.4 5L5 6.4 10.6 12 5 17.6 6.4 19l5.6-5.6 5.6 5.6 1.4-1.4-5.6-5.6L19 6.4 17.6 5 12 10.6z',
    up: 'M12 7l-6 6 1.4 1.4 4.6-4.6 4.6 4.6L18 13z',
    down: 'M12 17l6-6-1.4-1.4-4.6 4.6-4.6-4.6L6 11z',
    search:
      'M15.5 14h-.8l-.3-.3A6.5 6.5 0 0 0 16 9.5 6.5 6.5 0 1 0 9.5 16c1.6 0 3.1-.6 4.2-1.6l.3.3v.8l5 5 1.5-1.5-5-5zm-6 0A4.5 4.5 0 1 1 14 9.5 4.5 4.5 0 0 1 9.5 14z',
    queue: 'M4 6h12v2H4zm0 5h12v2H4zm0 5h8v2H4zm14-5v6.2a3 3 0 1 0 2 2.8V8h2V6h-4z',
    disc: 'M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zm0 12.5a3.5 3.5 0 1 1 0-7 3.5 3.5 0 0 1 0 7zm0-5a1.5 1.5 0 1 0 0 3 1.5 1.5 0 0 0 0-3z',
    settings:
      'M19.4 13a7.6 7.6 0 0 0 0-2l2.1-1.6-2-3.5-2.5 1a7.4 7.4 0 0 0-1.7-1L15 3h-4l-.4 2.9a7.4 7.4 0 0 0-1.7 1l-2.5-1-2 3.5L6.6 11a7.6 7.6 0 0 0 0 2l-2.1 1.6 2 3.5 2.5-1c.5.4 1.1.7 1.7 1L11 21h4l.4-2.9c.6-.3 1.2-.6 1.7-1l2.5 1 2-3.5zM13 15.5a3.5 3.5 0 1 1 0-7 3.5 3.5 0 0 1 0 7z',
    back: 'M20 11H7.8l5.6-5.6L12 4l-8 8 8 8 1.4-1.4L7.8 13H20z',
    clock:
      'M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zm0 16a7 7 0 1 1 0-14 7 7 0 0 1 0 14zm.5-11H11v5l4.2 2.5.8-1.2-3.5-2.1z',
  }
</script>

<svg width={size} height={size} viewBox="0 0 24 24" aria-hidden="true" fill="currentColor">
  <path d={paths[name]} />
</svg>
