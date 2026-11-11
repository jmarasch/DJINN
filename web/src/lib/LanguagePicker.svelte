<script lang="ts">
  // The app's language on this device: the house's default (Admin >
  // System) or the phone's own, or one picked here.
  import { house, languages, locale, setLanguage, t } from './i18n.svelte'

  let chosen = $state(readChosen())
  function readChosen(): string {
    try {
      return localStorage.getItem('djinn-language') ?? ''
    } catch {
      return ''
    }
  }
</script>

{#if languages.length > 1}
  <label class="language">
    <span class="muted small">{t('language.label')}</span>
    <select
      bind:value={chosen}
      onchange={() => setLanguage(chosen)}
      aria-label={t('language.label')}
      lang={locale.code}
    >
      <option value=""
        >{house.code
          ? t('language.house', { name: languages.find((l) => l.code === house.code)?.name ?? house.code })
          : t('language.phone')}</option
      >
      {#each languages as language (language.code)}
        <option value={language.code} lang={language.code}>{language.name}</option>
      {/each}
    </select>
  </label>
{/if}

<style>
  .language {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }

  .language select {
    min-height: 32px;
    padding: 0 8px;
    font-size: 0.8125rem;
  }
</style>
