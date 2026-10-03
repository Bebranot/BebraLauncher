<script lang="ts">
  import { app } from "./store.svelte";
  import { runLevelText, serverTitle } from "./format";

  const MAX_TAGS = 3;
</script>

<section>
  <div class="tabs" role="tablist">
    {#each [["all", "Все"], ["favorites", "Избранное"], ["recent", "Недавние"]] as [id, label]}
      <button
        role="tab"
        class:on={app.filter === id}
        aria-selected={app.filter === id}
        onclick={() => (app.filter = id as typeof app.filter)}
      >
        {label}
      </button>
    {/each}
  </div>
  <div class="toolbar">
    <input
      type="text"
      class="search"
      placeholder="Поиск по названию, адресу, тегам…"
      bind:value={app.search}
    />
    {#if app.languages.length > 0}
      <select bind:value={app.language} aria-label="Язык">
        <option value="">Любой язык</option>
        {#each app.languages as l}<option value={l}>{l}</option>{/each}
      </select>
    {/if}
    <select bind:value={app.sort} aria-label="Сортировка" disabled={app.filter === "recent"}>
      <option value="players">По онлайну</option>
      <option value="name">По названию</option>
    </select>
    <label class="check">
      <input type="checkbox" bind:checked={app.hideEmpty} /> Без пустых
    </label>
    <button onclick={() => app.refreshServers()} disabled={app.serversLoading}>
      {app.serversLoading ? "Обновление…" : "Обновить"}
    </button>
  </div>

  {#if app.serversError}
    <div class="notice error">
      Не удалось обновить список: {app.serversError}
    </div>
  {/if}

  <div class="list" role="listbox" aria-label="Серверы">
    {#each app.visible as s (s.address)}
      {@const level = runLevelText(s.run_level)}
      <button
        class="row"
        class:active={s.address === app.selectedAddress}
        role="option"
        aria-selected={s.address === app.selectedAddress}
        onclick={() => (app.selectedAddress = s.address)}
        ondblclick={() => {
          app.selectedAddress = s.address;
          void app.startPlay();
        }}
      >
        <span
          class="star"
          class:on={app.favorites.has(s.address)}
          role="button"
          tabindex="-1"
          title={app.favorites.has(s.address) ? "Убрать из избранного" : "В избранное"}
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
            {[level, s.map].filter(Boolean).join(" · ")}
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
          Загрузка списка серверов…
        {:else if app.filter === "favorites" && app.servers.length > 0}
          Пока пусто: отметьте сервер звёздочкой.
        {:else if app.filter === "recent" && app.servers.length > 0}
          Вы ещё не играли на серверах из этого списка.
        {:else if app.servers.length === 0}
          Список пуст.
        {:else}
          Ничего не найдено.
        {/if}
      </div>
    {/each}
  </div>

  <footer class="dim">
    {app.visible.length} из {app.servers.length} серверов
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
    padding: 8px 16px;
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
