<script lang="ts">
  // Admin > Library Editor: the editor opens in a window of its own, where
  // the admin unlocks it every time it's opened.
  import { t } from './i18n.svelte'
  import { openEditorWindow } from './editorJump.svelte'
</script>

<section class="card wide">
  <h2>{t('admin.menu.editor')}</h2>
  <p class="small muted">
    {t('le.intro')}
  </p>
  <button class="primary" onclick={() => openEditorWindow()}>{t('le.open')}</button>
</section>
