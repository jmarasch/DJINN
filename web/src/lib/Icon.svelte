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
    | 'list'
    | 'grid'
    | 'more'
    | 'theme'
    | 'folder'
    | 'playlist'
    | 'qr'
    | 'shuffle'
    | 'ordered'

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
    more: 'M12 8a2 2 0 1 0 0-4 2 2 0 0 0 0 4zm0 6a2 2 0 1 0 0-4 2 2 0 0 0 0 4zm0 6a2 2 0 1 0 0-4 2 2 0 0 0 0 4z',
    theme: 'M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zm0 2v14a7 7 0 0 1 0-14z',
    folder: 'M10 4H4a2 2 0 0 0-2 2v12a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-8z',
    playlist: 'M3 6h12v2H3zm0 4h12v2H3zm0 4h8v2H3zm14-4v5.2a3 3 0 1 0 2 2.8V12h3v-2z',
    qr: 'M3 3h8v8H3zm2 2v4h4V5zm8-2h8v8h-8zm2 2v4h4V5zM3 13h8v8H3zm2 2v4h4v-4zm8-2h2v2h-2zm2 2h2v2h-2zm2-2h2v2h-2zm2 2h2v2h-2zm-6 2h2v2h-2zm4 0h2v2h-2zm2 2h2v2h-2zm-4 0h2v2h-2z',
    shuffle:
      'M10.6 9.2 5.4 4 4 5.4l5.2 5.2zM14.5 4l2 2L4 18.6 5.4 20 18 7.5l2 2V4zm.3 9.4-1.4 1.4 3.1 3.1-2 2H20v-5.5l-2 2z',
    ordered: 'M7 7h10v3l4-4-4-4v3H5v6h2zm10 10H7v-3l-4 4 4 4v-3h12v-6h-2z',
    list: 'M4 5h3v3H4zm5 0h11v3H9zM4 10.5h3v3H4zm5 0h11v3H9zM4 16h3v3H4zm5 0h11v3H9z',
    grid: 'M4 4h7v7H4zm9 0h7v7h-7zM4 13h7v7H4zm9 0h7v7h-7z',
    clock:
      'M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zm0 16a7 7 0 1 1 0-14 7 7 0 0 1 0 14zm.5-11H11v5l4.2 2.5.8-1.2-3.5-2.1z',
  }
</script>

<svg width={size} height={size} viewBox="0 0 24 24" aria-hidden="true" fill="currentColor">
  <path d={paths[name]} />
</svg>
