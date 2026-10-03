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
      <h3>{t("servers.fromLink")}</h3>
      <div class="adhoc-table">
        <ServerTable rows={[app.adhocRow]} sortable={false} />
      </div>
    </div>
  {/if}

  <ServerTable rows={app.visible}>
    {#if app.visible.length === 0}
      <p class="empty sub">
        {#if app.serversLoading && app.servers.length === 0}
          {t("servers.loading")}
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

  <div class="rule"></div>
  <div class="bar">
    <input
      type="text"
      class="search"
      placeholder={t("servers.search")}
      bind:value={app.search}
      bind:this={searchInput}
    />
    <button
      class="btn open-r"
      class:on={app.filtersOpen}
      onclick={() => (app.filtersOpen = !app.filtersOpen)}
    >
      {t("servers.filters", { shown: app.visible.length, total: app.servers.length })}
    </button>
    <button
      class="btn icon open-l"
      title={t("servers.refresh")}
      aria-label={t("servers.refresh")}
      disabled={app.serversLoading}
      onclick={() => app.refreshServers()}
    >
      <Icon name="refresh" />
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
  .note {
    padding: 4px 14px;
    font-size: 13px;
    border-bottom: 1px solid var(--scroll-sep);
  }
  .adhoc {
    padding: 6px 0 0;
    border-bottom: 2px solid var(--accent);
  }
  .adhoc h3 {
    padding: 0 14px 2px;
  }
  .adhoc-table {
    display: flex;
    max-height: 260px;
  }
  .empty {
    margin: 0;
    padding: 28px 16px;
    text-align: center;
  }
  .filters {
    flex: none;
    height: 210px;
  }
  .bar {
    flex: none;
    display: flex;
    align-items: center;
    padding: 4px 4px 0;
    min-height: 36px;
  }
  .search {
    flex: 1;
    min-width: 0;
    margin-right: 8px;
  }
  .btn.on {
    background: var(--ctrl-high);
  }
</style>
