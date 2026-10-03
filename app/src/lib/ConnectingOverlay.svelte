<script lang="ts">
  import { api } from "./api";
  import { formatBytes, stageText } from "./format";
  import { hasKey, t, type Key } from "./i18n.svelte";
  import { app } from "./store.svelte";

  const play = $derived(app.play);
  const preparing = $derived(play.phase === "preparing" ? play : null);
  const percent = $derived(
    preparing && preparing.total > 0
      ? Math.min(100, (preparing.done / preparing.total) * 100)
      : null,
  );
  const visible = $derived(
    play.phase === "preparing" || play.phase === "failed" || play.phase === "exited",
  );

  /** The translated headline for a failure code; unknown codes get the general one. */
  function failText(code: string, detail: string): string {
    const key = `fail.${code}`;
    // An expired login names the account.
    const base = t(hasKey(key) ? (key as Key) : "fail.other");
    return code === "account_expired" && detail ? `${detail}: ${base}` : base;
  }
</script>

{#if visible}
  <div class="backdrop" role="alertdialog" aria-modal="true" aria-label={t("conn.title")}>
    <div class="box">
      {#if preparing}
        <h2>{t("conn.title")}</h2>

        {#if app.redial}
          <div class="redial">
            <b>{t("conn.redial")}</b>
            {#if app.redial.reason}
              <div class="reason">{t("conn.redialReason", { reason: app.redial.reason })}</div>
            {/if}
          </div>
        {/if}

        <p class="status">{stageText(preparing.stage)}</p>
        <progress max="100" value={percent ?? undefined}></progress>
        <div class="foot">
          <button class="btn" onclick={() => app.cancelPlay()} disabled={preparing.stage === "starting"}>
            {t("conn.cancel")}
          </button>
          {#if percent !== null}
            <span class="sub">
              {#if preparing.unit === "bytes"}
                {formatBytes(preparing.done)} / {formatBytes(preparing.total)}
              {:else}
                {Math.round(percent)}%
              {/if}
            </span>
          {/if}
        </div>
      {:else if play.phase === "failed"}
        <h2>{t("conn.title")}</h2>
        <p class="status error">{failText(play.code, play.message)}</p>
        {#if play.message && play.code !== "account_expired"}
          <p class="detail sub">{play.message}</p>
        {/if}
        <div class="foot center">
          <button class="btn" onclick={() => app.dismissPlay()}>{t("conn.ok")}</button>
        </div>
      {:else if play.phase === "exited"}
        <h2>{t("conn.title")}</h2>
        <p class="status error">
          {play.code !== null ? t("game.crashedCode", { code: play.code }) : t("game.crashed")}
        </p>
        {#if play.logTail.length > 0}
          <pre>{play.logTail.join("\n")}</pre>
        {/if}
        <div class="foot center">
          <button class="btn" onclick={() => api.openLogsFolder()}>{t("game.openLogs")}</button>
          <button class="btn" onclick={() => app.dismissPlay()}>{t("conn.ok")}</button>
        </div>
      {/if}
    </div>
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 30;
    display: grid;
    place-items: center;
    background: var(--overlay);
  }
  .box {
    width: min(460px, calc(100vw - 32px));
    background: var(--popup);
    border: 1px solid var(--ctrl-mid);
    padding: 18px 22px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    user-select: text;
  }
  h2 {
    text-align: center;
  }
  .status {
    margin: 6px 0;
    text-align: center;
    white-space: pre-wrap;
    word-break: break-word;
  }
  .detail {
    margin: 0;
    font-size: 13px;
    text-align: center;
    word-break: break-word;
  }
  progress {
    width: 100%;
  }
  .foot {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 10px;
    margin-top: 4px;
  }
  .foot.center {
    justify-content: center;
  }
  .redial {
    padding: 8px 10px;
    border: 1px solid var(--warn);
    font-size: 14px;
  }
  .reason {
    margin-top: 4px;
    white-space: pre-wrap;
    word-break: break-word;
  }
  pre {
    margin: 0;
    max-height: 170px;
    overflow: auto;
    padding: 8px;
    background: var(--bg);
    font-size: 12px;
    white-space: pre-wrap;
    word-break: break-all;
  }
</style>
