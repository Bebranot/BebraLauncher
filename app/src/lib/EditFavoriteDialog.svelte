<script lang="ts">
  import { api } from "./api";
  import { t } from "./i18n.svelte";
  import Modal from "./Modal.svelte";
  import { app } from "./store.svelte";

  let {
    address,
    currentName = "",
    onclose,
  }: {
    address: string;
    currentName?: string | null;
    onclose: () => void;
  } = $props();

  let newAddress = $state("");
  let name = $state("");
  let invalid = $state(false);
  let fetching = $state(false);
  let input = $state<HTMLInputElement>();

  $effect(() => {
    newAddress = address;
    name = currentName ?? "";
  });

  $effect(() => input?.focus());

  async function fetchName() {
    const addr = newAddress.trim();
    if (!addr) return;
    fetching = true;
    try {
      const known = app.servers.find((s) => s.address === addr);
      if (known?.name) {
        name = known.name;
        return;
      }
      const pings = await api.pingServers([addr]);
      if (pings[0]?.status?.name) {
        name = pings[0].status.name;
      }
    } catch {
      // Ignored
    } finally {
      fetching = false;
    }
  }

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    const trimmedAddr = newAddress.trim();
    if (!trimmedAddr) return;
    try {
      await app.editFavorite(address, trimmedAddr, name.trim() || null);
      onclose();
    } catch {
      invalid = true;
    }
  }
</script>

<Modal title={t("server.editFav")} {onclose}>
  <form onsubmit={submit}>
    <label for="fav-address">{t("addfav.address")}</label>
    <input
      id="fav-address"
      type="text"
      placeholder={t("direct.placeholder")}
      bind:value={newAddress}
      bind:this={input}
      oninput={() => (invalid = false)}
      spellcheck="false"
      autocomplete="off"
    />

    <label for="fav-name">{t("server.favName")}</label>
    <div class="name-row">
      <input
        id="fav-name"
        type="text"
        placeholder={t("server.favName")}
        bind:value={name}
        spellcheck="false"
        autocomplete="off"
      />
      <button
        type="button"
        class="btn soft"
        disabled={fetching || !newAddress.trim()}
        onclick={fetchName}
      >
        {fetching ? "..." : t("server.fetchName")}
      </button>
    </div>

    {#if invalid}<div class="error">{t("addfav.invalid")}</div>{/if}

    <div class="actions">
      <button class="btn primary" type="submit" disabled={!newAddress.trim()}>
        {t("server.saveNote")}
      </button>
      <button class="btn ghost" type="button" onclick={onclose}>
        {t("server.cancelNote")}
      </button>
    </div>
  </form>
</Modal>

<style>
  form {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .name-row {
    display: flex;
    gap: 8px;
  }
  .name-row input {
    flex: 1;
  }
  .actions {
    display: flex;
    gap: 8px;
    margin-top: 6px;
  }
</style>
