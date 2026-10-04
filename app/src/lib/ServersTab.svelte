<script lang="ts">
  import { activeFilterCount } from "./filters";
  import FiltersPanel from "./FiltersPanel.svelte";
  import Icon from "./Icon.svelte";
  import { t } from "./i18n.svelte";
  import ServerTable from "./ServerTable.svelte";
  import { app } from "./store.svelte";

  let { searchInput = $bindable() }: { searchInput?: HTMLInputElement } = $props();

  const filtered = $derived(activeFilterCount(app.filters) > 0 || app.search.trim() !== "");
</script>

<div class="tab">
  {#if app.serversError}
    <div class="note error">{t("servers.error", { error: app.serversError })}</div>
  {:else if app.serversFromCache}
    <div class="note sub">{t("servers.cached")}</div>
  {/if}

  {#if app.adhocRow}
    <div class="adhoc">
      <h3 class="eyebrow">{t("servers.fromLink")}</h3>
      <div class="adhoc-table">
        <ServerTable rows={[app.adhocRow]} sortable={false} />
      </div>
    </div>
  {/if}

  <ServerTable rows={app.visible}>
    {#if app.visible.length === 0}
      <p class="empty sub">
        {#if app.serversLoading && app.servers.length === 0}
          <span class="spinner"></span>
          <span>{t("servers.loading")}</span>
        {:else if app.servers.length === 0}
          {t("servers.none")}
        {:else}
          {t("servers.noneFiltered")}
        {/if}
      </p>
    {/if}
  </ServerTable>

  {#if app.filtersOpen}
    <div class="filters"><FiltersPanel /></div>
  {/if}

  <div class="bar">
    <div class="searchbox">
      <span class="lens"><Icon name="search" size={16} /></span>
      <input
        type="text"
        class="search"
        placeholder={t("servers.search")}
        bind:value={app.search}
        bind:this={searchInput}
      />
    </div>
    <button
      class="btn open-r"
      class:on={app.filtersOpen}
      onclick={() => (app.filtersOpen = !app.filtersOpen)}
    >
      <Icon name="filter" size={16} />
      {t("servers.filters", { shown: app.visible.length, total: app.servers.length })}
    </button>
    <button
      class="btn icon open-l"
      title={t("servers.refresh")}
      aria-label={t("servers.refresh")}
      disabled={app.serversLoading}
      onclick={() => app.refreshServers()}
    >
      {#if app.serversLoading}<span class="spinner small"></span>{:else}<Icon name="refresh" size={18} />{/if}
    </button>
  </div>
</div>

<style>
  .tab {
    display: flex;
    flex-direction: column;
    min-height: 0;
    height: 100%;
  }
  /* Like in the official launcher, the search, the filters and the refresh sit under the list. */
  .bar {
    flex: none;
    display: flex;
    align-items: center;
    padding: 8px 14px;
    border-top: 1px solid var(--line);
  }
  .searchbox {
    margin-right: 8px;
  }
  .searchbox {
    position: relative;
    flex: 1;
    min-width: 0;
  }
  .lens {
    position: absolute;
    left: 10px;
    top: 50%;
    transform: translateY(-50%);
    color: var(--muted);
    pointer-events: none;
    display: grid;
  }
  .search {
    width: 100%;
    padding-left: 32px;
  }
  .spinner.small {
    width: 16px;
    height: 16px;
  }
  .note {
    padding: 4px 14px 8px;
    font-size: 13px;
  }
  .adhoc {
    padding: 4px 0 0;
    border-bottom: 1px solid var(--line);
  }
  .adhoc h3 {
    padding: 0 18px 2px;
  }
  .adhoc-table {
    display: flex;
    max-height: 260px;
  }
  .empty {
    margin: 0;
    padding: 36px 16px;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 10px;
    text-align: center;
  }
  .filters {
    flex: none;
    height: min(240px, 42vh);
    margin: 8px 14px 0;
  }
</style>
