<script lang="ts">
  // Admin > Library Editor: the editor opens in a window of its own, where
  // the admin unlocks it every time it's opened.
  import { openEditorWindow } from './editorJump.svelte'
</script>

<section class="card wide">
  <h2>Library Editor</h2>
  <p class="small muted">
    Change tags, covers, file names and where files are kept, compare an album with MusicBrainz, Deezer, Apple or
    Spotify, and look after the folders DJINN may change. It opens in a window of its own, full width, and asks for your
    password every time it opens, so the rest of Admin stays out of the way.
  </p>
  <button class="primary" onclick={() => openEditorWindow()}>Open the library editor</button>
</section>
