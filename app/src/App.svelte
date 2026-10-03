<script lang="ts">
  import { onMount, tick } from "svelte";
  import ConnectingOverlay from "./lib/ConnectingOverlay.svelte";
  import DialogHost from "./lib/DialogHost.svelte";
  import ErpOverlay from "./lib/ErpOverlay.svelte";
  import FavoritesTab from "./lib/FavoritesTab.svelte";
  import Header from "./lib/Header.svelte";
  import NewsTab from "./lib/NewsTab.svelte";
  import OptionsTab from "./lib/OptionsTab.svelte";
  import RecentTab from "./lib/RecentTab.svelte";
  import ServersTab from "./lib/ServersTab.svelte";
  import UpdatesTab from "./lib/UpdatesTab.svelte";
  import Toast from "./lib/Toast.svelte";
  import { app } from "./lib/store.svelte";

  let searchInput = $state<HTMLInputElement>();

  onMount(() => {
    void app.init();
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
    } else if (e.key === "Escape" && el === searchInput) {
      app.search = "";
      searchInput?.blur();
    }
  }
</script>

<svelte:window {onkeydown} />

<div class="shell">
  <Header />
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
        {:else}
          <OptionsTab />
        {/if}
      </div>
    {/key}
  </main>
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
  main {
    flex: 1;
    min-height: 0;
  }
  .page {
    height: 100%;
    animation: fade-in 0.16s ease-out;
  }
</style>
