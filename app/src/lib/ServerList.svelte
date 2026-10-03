<script lang="ts">
  import { formatPlayed, runLevelText, serverTitle } from "./format";
  import { t, type Key } from "./i18n.svelte";
  import { app } from "./store.svelte";

  const MAX_TAGS = 3;
  const TABS: [typeof app.filter, Key][] = [
    ["all", "tab.all"],
    ["favorites", "tab.favorites"],
    ["recent", "tab.recent"],
  ];

  let { searchInput = $bindable() }: { searchInput?: HTMLInputElement } = $props();
</script>

<section>
  <div class="tabs" role="tablist">
    {#each TABS as [id, label]}
      <button
        role="tab"
        class:on={app.filter === id}
        aria-selected={app.filter === id}
        onclick={() => (app.filter = id)}
      >
        {t(label)}
      </button>
    {/each}
  </div>
  <div class="toolbar">
    <input
      type="text"
      class="search"
      placeholder={t("list.search")}
      bind:value={app.search}
      bind:this={searchInput}
    />
    {#if app.languages.length > 0}
      <select bind:value={app.language} aria-label={t("list.language")}>
        <option value="">{t("list.anyLanguage")}</option>
        {#each app.languages as l}<option value={l}>{l}</option>{/each}
      </select>
    {/if}
    {#if app.regions.length > 0}
      <select bind:value={app.region} aria-label={t("list.region")}>
        <option value="">{t("list.anyRegion")}</option>
        {#each app.regions as r}<option value={r}>{r}</option>{/each}
      </select>
    {/if}
    <select
      bind:value={app.sort}
      aria-label={t("list.sort")}
      disabled={app.filter === "recent"}
    >
      <option value="players">{t("list.sortPlayers")}</option>
      <option value="name">{t("list.sortName")}</option>
    </select>
    <label class="check">
      <input type="checkbox" bind:checked={app.hideEmpty} />
      {t("list.hideEmpty")}
    </label>
    <label class="check">
      <input type="checkbox" bind:checked={app.hideFull} />
      {t("list.hideFull")}
    </label>
    <button onclick={() => app.refreshServers()} disabled={app.serversLoading}>
      {app.serversLoading ? t("list.refreshing") : t("list.refresh")}
    </button>
  </div>

  {#if app.serversError}
    <div class="notice error">{t("list.refreshError", { error: app.serversError })}</div>
  {:else if app.serversFromCache}
    <div class="notice dim">{t("list.cached")}</div>
  {/if}

  <div class="list" role="listbox" aria-label={t("list.listbox")}>
    {#each app.visible as s (s.address)}
      {@const level = runLevelText(s.run_level)}
      {@const played = app.filter === "recent" ? app.historyOf(s.address) : undefined}
      <button
        class="row"
        class:active={s.address === app.selectedAddress}
        role="option"
        aria-selected={s.address === app.selectedAddress}
        onclick={() => app.select(s.address)}
        ondblclick={() => {
          app.select(s.address);
          void app.startPlay();
        }}
      >
        <span
          class="star"
          class:on={app.favorites.has(s.address)}
          role="button"
          tabindex="-1"
          title={app.favorites.has(s.address) ? t("list.starRemove") : t("list.starAdd")}
          onclick={(e) => {
            e.stopPropagation();
            void app.toggleFavorite(s.address);
          }}
          onkeydown={() => {}}
          ondblclick={(e) => e.stopPropagation()}
        >
          {app.favorites.has(s.address) ? "★" : "☆"}
        </span>
        <span class="main">
          <span class="name">{serverTitle(s)}</span>
          <span class="meta dim">
            {[level, s.map, played && played.play_seconds > 0 ? formatPlayed(played.play_seconds) : null]
              .filter(Boolean)
              .join(" · ")}
          </span>
        </span>
        <span class="tags">
          {#each s.tags.slice(0, MAX_TAGS) as tag}
            <span class="tag">{tag}</span>
          {/each}
        </span>
        <span class="players" class:none={s.players === 0}>
          {s.players}{#if s.soft_max_players > 0}<span class="dim"> / {s.soft_max_players}</span>{/if}
        </span>
      </button>
    {:else}
      <div class="empty dim">
        {#if app.serversLoading && app.servers.length === 0}
          {t("list.loading")}
        {:else if app.filter === "favorites" && app.servers.length > 0}
          {t("list.favEmpty")}
        {:else if app.filter === "recent" && app.servers.length > 0}
          {t("list.recentEmpty")}
        {:else if app.servers.length === 0}
          {t("list.empty")}
        {:else}
          {t("list.nothing")}
        {/if}
      </div>
    {/each}
  </div>

  <footer class="dim">
    {t("list.count", { shown: app.visible.length, total: app.servers.length })}
  </footer>
</section>

<style>
  section {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    height: 100%;
  }
  .tabs {
    display: flex;
    gap: 4px;
    padding: 10px 16px 0;
  }
  .tabs button {
    background: none;
    border: none;
    border-bottom: 2px solid transparent;
    border-radius: 0;
    padding: 6px 12px;
    color: var(--text-dim);
  }
  .tabs button.on {
    color: var(--text);
    border-bottom-color: var(--accent);
  }
  .toolbar {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    align-items: center;
    padding: 12px 16px;
    border-bottom: 1px solid var(--border);
  }
  .search {
    flex: 1 1 220px;
    min-width: 0;
  }
  .check {
    display: flex;
    align-items: center;
    gap: 6px;
    white-space: nowrap;
    color: var(--text-dim);
  }
  .notice {
    padding: 6px 16px;
    font-size: 13px;
    border-bottom: 1px solid var(--border);
  }
  .list {
    flex: 1;
    overflow-y: auto;
    padding: 8px;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .row {
    flex: none;
    display: flex;
    align-items: center;
    gap: 12px;
    width: 100%;
    text-align: left;
    background: none;
    border: 1px solid transparent;
    border-radius: var(--radius);
    padding: 8px 10px;
  }
  .row:hover {
    background: var(--bg-hover);
  }
  .row.active {
    background: var(--bg-active);
    border-color: var(--accent);
  }
  .star {
    flex: none;
    width: 22px;
    text-align: center;
    font-size: 17px;
    color: var(--text-dim);
    cursor: pointer;
  }
  .star:hover,
  .star.on {
    color: var(--warn);
  }
  .main {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: 600;
  }
  .meta {
    font-size: 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .tags {
    display: flex;
    gap: 4px;
    flex-shrink: 0;
  }
  .tag {
    font-size: 11px;
    padding: 1px 7px;
    border-radius: 10px;
    background: var(--bg);
    border: 1px solid var(--border);
    color: var(--text-dim);
    white-space: nowrap;
  }
  .players {
    min-width: 64px;
    text-align: right;
    font-variant-numeric: tabular-nums;
    font-weight: 600;
    color: var(--good);
  }
  .players.none {
    color: var(--text-dim);
  }
  .empty {
    padding: 32px;
    text-align: center;
  }
  footer {
    padding: 6px 16px;
    font-size: 12px;
    border-top: 1px solid var(--border);
  }
</style>
