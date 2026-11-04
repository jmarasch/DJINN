<script lang="ts">
  // Who can see one of your playlists: only you, everyone, or particular people.
  import { api } from './api'
  import { app, toast } from './state.svelte'
  import type { ShareTarget } from './types'

  let {
    id,
    shared,
    setShared,
  }: {
    id: number
    shared: boolean
    setShared: (shared: boolean) => unknown
  } = $props()

  let people = $state<ShareTarget[]>([])
  let chosen = $state<number[]>([])
  const canShare = $derived(!!app.me?.permissions?.share_playlists)

  $effect(() => {
    api
      .playlistShares(id)
      .then((list) => {
        chosen = list.map((person) => person.id)
        // An account that can no longer share can still stop sharing.
        people = list
        if (canShare) return api.shareTargets().then((all) => (people = all))
      })
      .catch((e) => toast(e.message, true))
  })

  async function toggle(person: ShareTarget, on: boolean) {
    try {
      if (on) {
        await api.sharePlaylist(id, person.id)
        chosen = [...chosen, person.id]
      } else {
        await api.unsharePlaylist(id, person.id)
        chosen = chosen.filter((other) => other !== person.id)
      }
    } catch (e) {
      toast(e instanceof Error ? e.message : 'Could not change that', true)
    }
  }
</script>

{#if canShare || shared || chosen.length}
  <details class="sharing">
    <summary>Sharing: {shared ? 'everyone' : chosen.length ? `${chosen.length} ${chosen.length === 1 ? 'person' : 'people'}` : 'only you'}</summary>
    <label>
      <input
        type="checkbox"
        checked={shared}
        disabled={!canShare && !shared}
        onchange={(e) => setShared(e.currentTarget.checked)}
      />
      Everyone on this server
    </label>
    {#each people as person (person.id)}
      <label>
        <input
          type="checkbox"
          checked={chosen.includes(person.id)}
          disabled={shared}
          onchange={(e) => toggle(person, e.currentTarget.checked)}
        />
        {person.name}
      </label>
    {/each}
    <p class="muted small">They can see and play it. Only you can change it.</p>
  </details>
{/if}

<style>
  .sharing {
    margin: 12px 0;
  }

  label {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 36px;
  }

  input {
    width: auto;
  }
</style>
