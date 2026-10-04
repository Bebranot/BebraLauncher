<script lang="ts">
  import Icon from "./Icon.svelte";
  import { t } from "./i18n.svelte";
  import ServerTable from "./ServerTable.svelte";
  import { app } from "./store.svelte";
</script>

<div class="tab">
  <div class="top">
    <h2>{t("fav.title")}</h2>
    <div class="buttons">
      <button
        class="btn icon open-r"
        title={t("fav.refresh")}
        aria-label={t("fav.refresh")}
        disabled={app.serversLoading}
        onclick={() => app.refreshServers()}
      >
        <Icon name="refresh" />
      </button>
      <button
        class="btn icon open-l"
        title={t("fav.add")}
        aria-label={t("fav.add")}
        onclick={() => (app.dialog = { kind: "addfav" })}
      >
        <Icon name="plus" />
      </button>
    </div>
  </div>
  {#if app.favoriteRows.length > 0}
    <ServerTable rows={app.favoriteRows} sortable={false} raise>
      <div class="more">
        <button class="btn" onclick={() => (app.tab = "servers")}>{t("fav.goServers")}</button>
      </div>
    </ServerTable>
  {:else}
    <div class="empty">
      <p>{t("fav.empty")}</p>
      <button class="btn" onclick={() => (app.tab = "servers")}>{t("fav.goServers")}</button>
    </div>
  {/if}

  <div class="bar">
    <button class="btn" onclick={() => (app.dialog = { kind: "direct" })}>
      {t("fav.direct")}
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
  .top {
    flex: none;
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px 14px 8px 18px;
  }
  .buttons {
    display: flex;
  }
  .more {
    display: flex;
    justify-content: center;
    padding: 14px;
  }
  .empty {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 10px;
    padding: 20px;
    text-align: center;
  }
  .empty p {
    margin: 0;
  }
  .bar {
    flex: none;
    display: flex;
    padding: 10px 14px;
    border-top: 1px solid var(--line);
  }
</style>
