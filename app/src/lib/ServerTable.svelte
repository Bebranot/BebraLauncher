<script lang="ts">
  import type { Snippet } from "svelte";
  import type { ServerRow } from "./api";
  import { nextSort, type SortKey } from "./filters";
  import { gridTemplate } from "./format";
  import { t } from "./i18n.svelte";
  import ServerEntry from "./ServerEntry.svelte";
  import { app } from "./store.svelte";

  let {
    rows,
    sortable = true,
    raise = false,
    children,
  }: {
    rows: ServerRow[];
    /** Clicking a column header sorts by it. */
    sortable?: boolean;
    raise?: boolean;
    /** Shown under the rows: an empty-list message or buttons. */
    children?: Snippet;
  } = $props();

  const columns = $derived(app.columns);
  const template = $derived(gridTemplate(columns));

  function sortBy(key: SortKey) {
    if (sortable) app.sort = nextSort(app.sort, key);
  }
</script>

{#snippet head(key: SortKey, label: string, center: boolean)}
  <button
    class="cell hbtn"
    class:center
    disabled={!sortable}
    onclick={() => sortBy(key)}
  >
    {label}{#if sortable && app.sort.key === key}<span class="arrow">{app.sort.desc ? "▼" : "▲"}</span>{/if}
  </button>
{/snippet}

<div class="table">
  <div class="header" style:grid-template-columns={template}>
    {@render head("name", t("col.name"), false)}
    {#if columns.mode}{@render head("mode", t("col.mode"), true)}{/if}
    {#if columns.map}{@render head("map", t("col.map"), true)}{/if}
    {#if columns.ping}{@render head("ping", t("col.ping"), true)}{/if}
    {#if columns.round_time}{@render head("time", t("col.time"), true)}{/if}
    {#if columns.players}{@render head("players", t("col.players"), true)}{/if}
    <div class="cell last"></div>
  </div>

  <div class="scroll">
    {#each rows as server (server.address)}
      <ServerEntry {server} {raise} />
    {/each}
    {@render children?.()}
  </div>
</div>

<style>
  .table {
    display: flex;
    flex-direction: column;
    min-height: 0;
    flex: 1;
    background: var(--decor);
  }
  .header {
    display: grid;
    padding: 0 10px 0 14px; /* the right side is the width of the scrollbar below */
    border-bottom: 1px solid var(--line);
  }
  .cell {
    padding: 8px 10px;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--sub);
  }
  .cell.center {
    text-align: center;
  }
  .hbtn {
    appearance: none;
    background: none;
    border: 0;
    font: inherit;
    letter-spacing: inherit;
    text-transform: inherit;
    text-align: left;
    cursor: pointer;
  }
  .hbtn.center {
    text-align: center;
  }
  .hbtn:disabled {
    cursor: default;
  }
  .hbtn:hover:not(:disabled) {
    color: var(--fg);
  }
  .arrow {
    margin-left: 4px;
    font-size: 9px;
    color: var(--accent);
  }
  .scroll {
    flex: 1;
    min-height: 0;
    overflow-x: hidden;
    overflow-y: scroll;
    padding: 0 0 8px;
  }
</style>
