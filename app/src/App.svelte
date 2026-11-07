<script lang="ts">
  import { onDestroy, onMount, tick } from "svelte";
  import ConnectingOverlay from "./lib/ConnectingOverlay.svelte";
  import DialogHost from "./lib/DialogHost.svelte";
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
  import SetupWizard from "./lib/setup/SetupWizard.svelte";
  import Toast from "./lib/Toast.svelte";
  import { t } from "./lib/i18n.svelte";
  import { app } from "./lib/store.svelte";

  let searchInput = $state<HTMLInputElement>();

  /** The page of the current tab and the element that scrolls it. */
  let page = $state<HTMLElement>();
  let pageHost = $state<HTMLElement>();

  /** Where each tab was left: switching tabs must not throw the player back to the top. */
  const scrollPositions = new Map<string, number>();

  function scrollRoot(): HTMLElement | null {
    if (!page) return null;
    const marked = page.querySelector<HTMLElement>("[data-scroll]");
    if (marked) return marked;
    // Tabs that do not mark their scroller: the first element that actually scrolls.
    for (const el of page.querySelectorAll<HTMLElement>("*")) {
      const style = getComputedStyle(el);
      if (
        (style.overflowY === "auto" || style.overflowY === "scroll") &&
        el.scrollHeight > el.clientHeight
      ) {
        return el;
      }
    }
    return null;
  }

  function onPageScroll(e: Event) {
    const el = e.target;
    if (!(el instanceof HTMLElement) || !page?.contains(el)) return;
    scrollPositions.set(app.tab, el.scrollTop);
  }

  // After the new tab is in the DOM, put its scroll position back.
  $effect(() => {
    const tab = app.tab;
    void tick().then(() => {
      const el = scrollRoot();
      if (el) el.scrollTop = scrollPositions.get(tab) ?? 0;
    });
  });

  const isLowSpec = $derived(app.settings?.opt_profile === "optimized");
  $effect(() => {
    document.body.classList.toggle("low-spec", isLowSpec);
  });

  onMount(() => {
    void app.init();
    // Scroll does not bubble, so the listener has to sit in the capture phase.
    pageHost?.addEventListener("scroll", onPageScroll, true);
  });

  onDestroy(() => {
    pageHost?.removeEventListener("scroll", onPageScroll, true);
    app.dispose();
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
    // Any open modal owns the keyboard (its own trap handles Escape and Tab).
    if (document.querySelector('[role="dialog"][aria-modal="true"]')) return;
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
  {#if app.initError || app.loadError}
    <div class="load-alert" role="alert">
      <span class="load-text">
        {t("app.loadFailed", { error: app.initError ?? app.loadError ?? "" })}
      </span>
      <button class="btn small" onclick={() => void app.retryInit()}>{t("app.retry")}</button>
    </div>
  {/if}
  <main bind:this={pageHost}>
    <div
      class="page"
      bind:this={page}
      role="tabpanel"
      id={`panel-${app.tab}`}
      aria-labelledby={`tab-${app.tab}`}
    >
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
  </main>
  <TabBar />
</div>

<ConnectingOverlay />
<DialogHost />
{#if app.setupOpen}
  <SetupWizard />
{/if}
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
  .load-alert {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 10px 16px;
    background: color-mix(in srgb, var(--bad) 15%, transparent);
    border-bottom: 1px solid color-mix(in srgb, var(--bad) 40%, transparent);
    font-size: 13px;
    color: var(--fg);
    user-select: text;
  }
  .load-text {
    flex: 1;
    line-height: 1.4;
    word-break: break-word;
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
