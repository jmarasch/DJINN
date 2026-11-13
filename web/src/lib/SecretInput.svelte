<script lang="ts">
  // A field for an API key or other secret: hidden as dots, with an eye to
  // show it while you type or check it, so it isn't on screen by accident.
  import Icon from './Icon.svelte'

  let {
    value = $bindable(''),
    placeholder = '',
    label,
    maxlength,
  }: {
    value?: string
    placeholder?: string
    /** What the field is, for screen readers. */
    label: string
    maxlength?: number
  } = $props()

  let shown = $state(false)
</script>

<span class="secret">
  <input
    type={shown ? 'text' : 'password'}
    bind:value
    {placeholder}
    {maxlength}
    aria-label={label}
    autocomplete="off"
    autocapitalize="none"
    spellcheck="false"
  />
  <button
    type="button"
    class="reveal"
    aria-pressed={shown}
    aria-label={shown ? `Hide ${label}` : `Show ${label}`}
    title={shown ? 'Hide' : 'Show'}
    onclick={() => (shown = !shown)}
  >
    <Icon name={shown ? 'eye-off' : 'eye'} size={20} />
  </button>
</span>

<style>
  .secret {
    position: relative;
    display: flex;
    align-items: center;
  }

  .secret input {
    flex: 1;
    min-width: 0;
    width: 100%;
    padding-right: 48px;
  }

  .reveal {
    position: absolute;
    top: 0;
    right: 0;
    bottom: 0;
    width: 44px;
    min-height: 0;
    padding: 0;
    background: transparent;
    color: var(--muted);
  }

  .reveal:hover,
  .reveal[aria-pressed='true'] {
    color: var(--accent);
  }
</style>
