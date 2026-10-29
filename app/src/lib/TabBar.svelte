<script lang="ts">
  import { t, type Key } from "./i18n.svelte";
  import { app, type Tab } from "./store.svelte";

  const TABS: [Tab, Key][] = [
    ["favorites", "tab.favorites"],
    ["servers", "tab.servers"],
    ["recent", "tab.recent"],
    ["news", "tab.news"],
    ["updates", "tab.updates"],
    ["mods", "tab.mods"],
    ["options", "tab.options"],
  ];

  let buttons = $state<HTMLButtonElement[]>([]);

  /** Left/Right/Home/End move between the tabs, as a tab list is expected to (P2-55). */
  function onkeydown(e: KeyboardEvent, index: number) {
    const last = TABS.length - 1;
    let next = index;
    if (e.key === "ArrowRight") next = index === last ? 0 : index + 1;
    else if (e.key === "ArrowLeft") next = index === 0 ? last : index - 1;
    else if (e.key === "Home") next = 0;
    else if (e.key === "End") next = last;
    else return;
    e.preventDefault();
    app.tab = TABS[next][0];
    // Roving tabindex: the selected tab is the only stop, so focus follows the selection.
    queueMicrotask(() => buttons[next]?.focus());
  }
</script>

<!-- At the bottom of the window, like in the official launcher. -->
<div class="nav" role="tablist" aria-label={t("tab.listLabel")}>
  {#each TABS as [id, label], i (id)}
    <button
      bind:this={buttons[i]}
      id={`tab-${id}`}
      role="tab"
      aria-selected={app.tab === id}
      aria-controls={`panel-${id}`}
      tabindex={app.tab === id ? 0 : -1}
      class:selected={app.tab === id}
      onclick={() => (app.tab = id)}
      onkeydown={(e) => onkeydown(e, i)}
    >
      {t(label)}
      {#if id === "updates" && app.updateAvailable}<i class="dot" aria-hidden="true"></i>{/if}
    </button>
  {/each}
</div>

<style>
  .nav {
    flex: none;
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 6px 10px;
    border-top: 1px solid var(--line);
    overflow-x: auto;
    scrollbar-width: none;
  }
  button {
    appearance: none;
    position: relative;
    border: 0;
    border-radius: var(--radius-sm);
    background: none;
    padding: 8px 14px;
    font-size: 13px;
    font-weight: 500;
    cursor: pointer;
    color: var(--sub);
    white-space: nowrap;
    user-select: none;
    transition:
      background 0.12s ease,
      color 0.12s ease;
  }
  .dot {
    /* Over the corner of the label: it must not widen the tab. */
    position: absolute;
    top: 6px;
    right: 6px;
    width: 6px;
    height: 6px;
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
    left: 12px;
    right: 12px;
    bottom: 2px;
    height: 2px;
    border-radius: 1px;
    background: var(--accent);
  }

  /* Rollback for classic theme: exact official launcher bottom tab bar styling */
  :global(html[data-theme="classic"]) .nav {
    gap: 2px;
    padding: 0 8px;
    align-items: stretch;
  }
  :global(html[data-theme="classic"]) button {
    border-radius: 0;
    padding: 11px 14px;
    font-size: inherit;
    font-weight: inherit;
  }
  :global(html[data-theme="classic"]) button.selected::after {
    left: 0;
    right: 0;
    top: -1px;
    bottom: auto;
    border-radius: 0;
  }
  :global(html[data-theme="classic"]) .dot {
    top: 8px;
    right: 5px;
    width: 7px;
    height: 7px;
  }
</style>
