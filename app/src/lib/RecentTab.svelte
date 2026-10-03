<script lang="ts">
  import { t } from "./i18n.svelte";
  import ServerTable from "./ServerTable.svelte";
  import { app } from "./store.svelte";
</script>

<div class="tab">
  <div class="top"><h2>{t("recent.title")}</h2></div>
  {#if app.recentRows.length > 0}
    <ServerTable rows={app.recentRows} sortable={false} />
  {:else}
    <div class="empty sub">{t("recent.empty")}</div>
  {/if}
</div>

<style>
  .tab {
    display: flex;
    flex-direction: column;
    min-height: 0;
    height: 100%;
  }
  .top {
    flex: none;
    padding: 12px 18px 8px;
    display: flex;
    align-items: center;
  }
  .empty {
    flex: 1;
    display: grid;
    place-items: center;
    padding: 20px;
  }
</style>
