<script lang="ts">
  import { untrack } from "svelte";
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

  let flavorIndex = $state(0);
  $effect(() => {
    if (preparing) {
      untrack(() => {
        flavorIndex = Math.floor(Math.random() * 8);
      });
    }
  });
</script>

{#if visible}
  <div class="backdrop" role="alertdialog" aria-modal="true" aria-label={t("conn.title")}>
    <div class="box">
      {#if preparing}
        <h2><span class="spinner"></span>{t("conn.title")}</h2>

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
        <div class="flavor-tip sub">{t(`conn.flavor.${flavorIndex}` as Key)}</div>
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
    backdrop-filter: blur(2px);
    animation: backdrop-in 0.15s ease-out;
  }
  .box {
    width: min(460px, calc(100vw - 32px));
    background: var(--popup);
    border: 1px solid var(--line);
    border-radius: 12px;
    box-shadow: var(--shadow);
    animation: pop-in 0.16s ease-out;
    padding: 22px 24px;
    display: flex;
    flex-direction: column;
    gap: 10px;
    user-select: text;
  }
  h2 {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 10px;
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
    padding: 8px 12px;
    border: 1px solid color-mix(in srgb, var(--warn) 50%, transparent);
    background: color-mix(in srgb, var(--warn) 10%, transparent);
    border-radius: var(--radius-sm);
    font-size: 13px;
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
    padding: 10px;
    background: var(--tint);
    border-radius: var(--radius-sm);
    font-size: 12px;
    white-space: pre-wrap;
    word-break: break-all;
  }
  .flavor-tip {
    margin-top: 6px;
    font-size: 12px;
    font-style: italic;
    text-align: center;
    opacity: 0.8;
    line-height: 1.4;
  }
</style>
