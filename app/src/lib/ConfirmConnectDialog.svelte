<script lang="ts">
  import type { ServerRow } from "./api";
  import { t } from "./i18n.svelte";
  import Modal from "./Modal.svelte";
  import { app } from "./store.svelte";

  let { server, onclose }: { server: ServerRow; onclose: () => void } = $props();

  function confirm() {
    onclose();
    void app.connect(server, true);
  }
</script>

<Modal title={t("confirm.title")} {onclose}>
  <div class="confirm-content">
    <div class="server-title">{server.name || server.address}</div>
    <div class="server-address sub small">{server.address}</div>

    <div class="details-grid">
      {#if server.players !== undefined}
        <div class="detail-item">
          <span class="sub">{t("col.players")}:</span>
          <b>{server.players} / {server.soft_max_players ?? "∞"}</b>
        </div>
      {/if}
      {#if server.map}
        <div class="detail-item">
          <span class="sub">{t("col.map")}:</span>
          <b>{server.map}</b>
        </div>
      {/if}
      {#if server.preset}
        <div class="detail-item">
          <span class="sub">{t("col.mode")}:</span>
          <b>{server.preset}</b>
        </div>
      {/if}
    </div>

    <p class="sub small prompt">{t("confirm.prompt")}</p>

    <div class="actions">
      <button class="btn primary" onclick={confirm} disabled={app.busy}>
        {t("server.connect")}
      </button>
      <button class="btn" onclick={onclose}>
        {t("conn.cancel")}
      </button>
    </div>
  </div>
</Modal>

<style>
  .confirm-content {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .server-title {
    font-size: 15px;
    font-weight: 600;
  }
  .server-address {
    word-break: break-all;
  }
  .details-grid {
    display: flex;
    flex-wrap: wrap;
    gap: 12px;
    padding: 8px 12px;
    background: var(--tint);
    border-radius: var(--radius-sm);
    border: 1px solid var(--line);
  }
  .detail-item {
    display: flex;
    gap: 6px;
    font-size: 13px;
  }
  .prompt {
    margin: 4px 0 0;
  }
  .actions {
    display: flex;
    gap: 8px;
    margin-top: 8px;
  }
</style>
