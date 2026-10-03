<script lang="ts">
  import { onMount } from "svelte";
  import AccountsModal from "./lib/AccountsModal.svelte";
  import DirectConnectModal from "./lib/DirectConnectModal.svelte";
  import { t } from "./lib/i18n.svelte";
  import ServerList from "./lib/ServerList.svelte";
  import ServerPanel from "./lib/ServerPanel.svelte";
  import SettingsModal from "./lib/SettingsModal.svelte";
  import { app } from "./lib/store.svelte";

  let dialog = $state<"accounts" | "settings" | "direct" | null>(null);
  let searchInput = $state<HTMLInputElement>();

  const account = $derived(app.accounts.find((a) => a.user_id === app.selectedAccount) ?? null);

  onMount(() => {
    void app.init();
  });

  /** Keyboard: `/` or Ctrl+F search, arrows move through the list, Enter plays. */
  function onkeydown(e: KeyboardEvent) {
    if (dialog) return;
    const el = e.target as HTMLElement | null;
    const typing =
      el instanceof HTMLInputElement || el instanceof HTMLSelectElement || el instanceof HTMLTextAreaElement;

    if ((e.key === "/" && !typing) || (e.key === "f" && e.ctrlKey)) {
      e.preventDefault();
      searchInput?.focus();
      searchInput?.select();
    } else if (e.key === "Escape" && el === searchInput) {
      app.search = "";
      searchInput?.blur();
    } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      if (el instanceof HTMLSelectElement) return;
      e.preventDefault();
      app.moveSelection(e.key === "ArrowDown" ? 1 : -1);
    } else if (e.key === "Enter" && !typing && !(el instanceof HTMLButtonElement)) {
      void app.startPlay();
    }
  }
</script>

<svelte:window {onkeydown} />

<div class="shell">
  <header>
    <div class="brand">BebraLauncher</div>
    <div class="spacer"></div>
    <button onclick={() => (dialog = "direct")}>{t("nav.direct")}</button>
    <button onclick={() => (dialog = "accounts")}>
      {account ? account.username : t("nav.signIn")}
    </button>
    <button onclick={() => (dialog = "settings")} aria-label={t("nav.settings")}>⚙</button>
  </header>

  <main>
    <ServerList bind:searchInput />
    <ServerPanel onmanageaccounts={() => (dialog = "accounts")} />
  </main>
</div>

{#if dialog === "accounts"}
  <AccountsModal onclose={() => (dialog = null)} />
{:else if dialog === "settings"}
  <SettingsModal onclose={() => (dialog = null)} />
{:else if dialog === "direct"}
  <DirectConnectModal onclose={() => (dialog = null)} />
{/if}

<style>
  .shell {
    display: flex;
    flex-direction: column;
    height: 100%;
  }
  header {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 16px;
    background: var(--bg-elev);
    border-bottom: 1px solid var(--border);
  }
  .brand {
    font-weight: 700;
    font-size: 16px;
    letter-spacing: 0.3px;
  }
  .spacer {
    flex: 1;
  }
  main {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: minmax(0, 1fr) 340px;
  }
</style>
