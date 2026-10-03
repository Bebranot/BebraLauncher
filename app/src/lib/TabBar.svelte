<script lang="ts">
  import { t, type Key } from "./i18n.svelte";
  import { app, type Tab } from "./store.svelte";

  const TABS: [Tab, Key][] = [
    ["favorites", "tab.favorites"],
    ["servers", "tab.servers"],
    ["recent", "tab.recent"],
    ["news", "tab.news"],
    ["options", "tab.options"],
  ];
</script>

<div class="tabs" role="tablist">
  {#each TABS as [id, label]}
    <button
      role="tab"
      aria-selected={app.tab === id}
      class:selected={app.tab === id}
      onclick={() => (app.tab = id)}
    >
      {t(label)}
    </button>
  {/each}
  <div class="grow"></div>
  {#if app.info}
    <div class="version sub">{t("header.version", { version: app.info.version })}</div>
  {/if}
</div>

<style>
  .tabs {
    display: flex;
    align-items: stretch;
    background: var(--popup);
    border-top: 1px solid var(--scroll-sep);
  }
  button {
    appearance: none;
    border: 0;
    background: none;
    padding: 8px 20px;
    cursor: pointer;
    color: var(--fg);
  }
  button:hover {
    background: var(--tab-hover);
  }
  button.selected,
  button.selected:hover {
    background: var(--tab-selected);
  }
  .grow {
    flex: 1;
  }
  .version {
    align-self: center;
    padding: 0 12px;
    font-size: 13px;
  }
</style>
