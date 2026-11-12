<script lang="ts">
  // Admin > System > Language: what every app shows (party app, DJINN GO on
  // the web and on Android) unless someone picks their own on their account
  // page.
  import { onMount } from 'svelte'
  import { api } from './api'
  import { house, languages, loadHouseLanguage } from './i18n.svelte'
  import { toast } from './state.svelte'

  let value = $state(house.code)
  let loaded = $state(false)

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
      toast(value ? `DJINN now shows ${languages.find((l) => l.code === value)?.name ?? value}` : 'Each device shows its own language')
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
    page. The admin pages stay in English.
  </p>
</section>

<style>
  .field {
    display: flex;
    flex-direction: column;
    gap: 6px;
    max-width: 320px;
  }
</style>
