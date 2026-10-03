<script lang="ts">
  import { t, type Key } from "./i18n.svelte";
  import { app, type Tab } from "./store.svelte";

  const TABS: [Tab, Key][] = [
    ["favorites", "tab.favorites"],
    ["servers", "tab.servers"],
    ["recent", "tab.recent"],
    ["news", "tab.news"],
    ["updates", "tab.updates"],
    ["options", "tab.options"],
  ];
</script>

<div class="nav" role="tablist">
  {#each TABS as [id, label]}
    <button
      role="tab"
      aria-selected={app.tab === id}
      class:selected={app.tab === id}
      onclick={() => (app.tab = id)}
    >
      {t(label)}
      {#if id === "updates" && app.updateAvailable}<i class="dot" aria-hidden="true"></i>{/if}
    </button>
  {/each}
</div>

<style>
  .nav {
    display: flex;
    align-items: center;
    gap: 0;
    margin-left: 4px;
    min-width: 0;
  }
  button {
    appearance: none;
    position: relative;
    border: 0;
    border-radius: var(--radius-sm);
    background: none;
    padding: 6px 8px;
    cursor: pointer;
    color: var(--sub);
    white-space: nowrap;
    transition:
      background 0.12s,
      color 0.12s;
  }
  .dot {
    /* Over the corner of the label: it must not widen the tab. */
    position: absolute;
    top: 5px;
    right: 1px;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--accent);
  }
  button:hover {
    background: var(--tint);
    color: var(--fg);
  }
  button.selected {
    color: var(--fg);
    background: var(--tint-hover);
  }
  button.selected::after {
    content: "";
    position: absolute;
    left: 8px;
    right: 8px;
    bottom: -11px;
    height: 2px;
    border-radius: 2px;
    background: var(--accent);
  }
</style>
