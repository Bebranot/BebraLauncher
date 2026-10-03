<script lang="ts">
  import { onMount } from "svelte";
  import AccountsModal from "./lib/AccountsModal.svelte";
  import ServerList from "./lib/ServerList.svelte";
  import ServerPanel from "./lib/ServerPanel.svelte";
  import SettingsModal from "./lib/SettingsModal.svelte";
  import { app } from "./lib/store.svelte";

  let dialog = $state<"accounts" | "settings" | null>(null);

  const account = $derived(app.accounts.find((a) => a.user_id === app.selectedAccount) ?? null);

  onMount(() => {
    void app.init();
  });
</script>

<div class="shell">
  <header>
    <div class="brand">BebraLauncher</div>
    <div class="spacer"></div>
    <button onclick={() => (dialog = "accounts")}>
      {account ? account.username : "Войти"}
    </button>
    <button onclick={() => (dialog = "settings")} aria-label="Настройки">⚙</button>
  </header>

  <main>
    <ServerList />
    <ServerPanel onmanageaccounts={() => (dialog = "accounts")} />
  </main>
</div>

{#if dialog === "accounts"}
  <AccountsModal onclose={() => (dialog = null)} />
{:else if dialog === "settings"}
  <SettingsModal onclose={() => (dialog = null)} />
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
