<script lang="ts">
  // Admin > System > Language: what every app shows (party app, DJINN GO on
  // the web and on Android) unless someone picks their own on their account
  // page.
  import { onMount } from 'svelte'
  import { api } from './api'
  import { house, languages, loadHouseLanguage, ownPick, setLanguage } from './i18n.svelte'
  import { toast } from './state.svelte'

  let value = $state(house.code)
  let loaded = $state(false)
  // This browser's own pick from Your Account, which wins over the house's.
  let own = $state(ownPick())
  const nameOf = (code: string) => languages.find((l) => l.code === code)?.name ?? code

  function followHouse() {
    setLanguage('')
    own = ''
  }

  onMount(async () => {
    try {
      value = (await api.language()).default
      loaded = true
    } catch {
      toast('Could not load the language setting', true)
    }
  })

  async function save() {
    try {
      await api.setLanguage(value)
      await loadHouseLanguage()
      toast(value ? `DJINN now shows ${nameOf(value)}` : 'Each device shows its own language')
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not save', true)
    }
  }
</script>

<section class="card">
  <h2>Language</h2>
  <label class="field">
    Default language
    <select bind:value onchange={save} disabled={!loaded}>
      <option value="">Each device's own language</option>
      {#each languages as language (language.code)}
        <option value={language.code}>{language.name}</option>
      {/each}
    </select>
  </label>
  <p class="small muted">
    What the party app, DJINN GO and the Android app show. Anyone can still pick their own language on their account
    page. The admin pages stay in English, apart from their menus.
  </p>
  {#if own}
    <p class="own small">
      This browser uses its own pick, {nameOf(own)}, from Your Account, so it won't change with this setting.
      <button class="link" onclick={followHouse}>Follow the default here</button>
    </p>
  {/if}
</section>

<style>
  .own {
    margin-top: 8px;
  }

  .link {
    background: none;
    border: none;
    padding: 0;
    min-height: 0;
    display: inline;
    line-height: inherit;
    color: var(--accent);
    text-decoration: underline;
    cursor: pointer;
    font: inherit;
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: 6px;
    max-width: 320px;
  }
</style>
