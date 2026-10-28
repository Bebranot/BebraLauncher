<script lang="ts">
  import { onMount, tick } from "svelte";
  import ConnectingOverlay from "./lib/ConnectingOverlay.svelte";
  import DialogHost from "./lib/DialogHost.svelte";
  import ErpOverlay from "./lib/ErpOverlay.svelte";
  import FavoritesTab from "./lib/FavoritesTab.svelte";
  import Header from "./lib/Header.svelte";
  import TabBar from "./lib/TabBar.svelte";
  import NewsTab from "./lib/NewsTab.svelte";
  import OptionsTab from "./lib/OptionsTab.svelte";
  import RecentTab from "./lib/RecentTab.svelte";
  import ServersTab from "./lib/ServersTab.svelte";
  import SpaceBackground from "./lib/SpaceBackground.svelte";
  import UpdatesTab from "./lib/UpdatesTab.svelte";
  import ModsTab from "./lib/ModsTab.svelte";
  import Toast from "./lib/Toast.svelte";
  import { promo } from "./lib/rofl.svelte";
  import { t } from "./lib/i18n.svelte";
  import { app } from "./lib/store.svelte";

  let searchInput = $state<HTMLInputElement>();

  onMount(() => {
    void app.init();
    // The joke sale's timer runs from the launch, whichever tab is open.
    promo.start();
  });

  /** `/` or Ctrl+F: search the server list. */
  async function focusSearch() {
    app.tab = "servers";
    await tick();
    searchInput?.focus();
    searchInput?.select();
  }

  function onkeydown(e: KeyboardEvent) {
    if (app.dialog || app.play.phase !== "idle") return;
    const el = e.target as HTMLElement | null;
    const typing =
      el instanceof HTMLInputElement ||
      el instanceof HTMLSelectElement ||
      el instanceof HTMLTextAreaElement;

    if ((e.key === "/" && !typing) || (e.key === "f" && e.ctrlKey)) {
      e.preventDefault();
      void focusSearch();
    } else if (e.key === "Escape") {
      if (el === searchInput || app.search !== "") {
        app.search = "";
        searchInput?.blur();
      } else if (app.filtersOpen) {
        app.filtersOpen = false;
      }
    } else if ((e.key === "F5" || (e.key === "r" && e.ctrlKey)) && !app.serversLoading) {
      if (app.tab === "servers" || app.tab === "favorites") {
        e.preventDefault();
        void app.refreshServers();
      }
    }
  }
</script>

<svelte:window {onkeydown} />

<SpaceBackground />
<div class="shell">
  <Header />
  {#if app.intelWarning}
    <div class="intel-alert" role="alert">
      <div class="intel-text">
        <strong>{t("opt.intelWarningTitle")}</strong>: {t("opt.intelWarningDesc")}
      </div>
      <button class="btn small" onclick={() => app.dismissIntelWarning()}>
        {t("opt.intelWarningDismiss")}
      </button>
    </div>
  {/if}
  <main>
    {#key app.tab}
      <div class="page">
        {#if app.tab === "favorites"}
          <FavoritesTab />
        {:else if app.tab === "servers"}
          <ServersTab bind:searchInput />
        {:else if app.tab === "recent"}
          <RecentTab />
        {:else if app.tab === "news"}
          <NewsTab />
        {:else if app.tab === "updates"}
          <UpdatesTab />
        {:else if app.tab === "mods"}
          <ModsTab />
        {:else}
          <OptionsTab />
        {/if}
      </div>
    {/key}
  </main>
  <TabBar />
</div>

<ConnectingOverlay />
<DialogHost />
<ErpOverlay />
<Toast />

<style>
  .shell {
    display: flex;
    flex-direction: column;
    height: 100%;
  }
  .intel-alert {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 10px 16px;
    background: color-mix(in srgb, var(--warn) 15%, transparent);
    border-bottom: 1px solid color-mix(in srgb, var(--warn) 40%, transparent);
    font-size: 13px;
    color: var(--fg);
    user-select: text;
  }
  .intel-text {
    flex: 1;
    line-height: 1.4;
  }
  main {
    flex: 1;
    min-height: 0;
  }
  .page {
    height: 100%;
    animation: fade-in 0.16s ease-out;
  }
</style>
