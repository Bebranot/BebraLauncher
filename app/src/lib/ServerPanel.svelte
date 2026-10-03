<script lang="ts">
  import { api } from "./api";
  import {
    formatBytes,
    formatPlayed,
    roundDuration,
    runLevelText,
    serverTitle,
    stageText,
  } from "./format";
  import { hasKey, t, type Key } from "./i18n.svelte";
  import { app } from "./store.svelte";

  let { onmanageaccounts }: { onmanageaccounts: () => void } = $props();

  const server = $derived(app.selected);
  const details = $derived(app.selectedDetails);
  const ready = $derived(details && details !== "loading" && details !== "failed" ? details : null);
  const account = $derived(app.accounts.find((a) => a.user_id === app.selectedAccount) ?? null);
  const needsAccount = $derived(ready?.auth === "required" && !account);
  const playable = $derived(
    !!server &&
      !app.busy &&
      !app.info?.loader_problem &&
      account?.state !== "expired" &&
      !needsAccount,
  );
  const played = $derived(server ? app.historyOf(server.address) : undefined);

  const progress = $derived(app.play.phase === "preparing" ? app.play : null);
  const percent = $derived(
    progress && progress.total > 0 ? Math.min(100, (progress.done / progress.total) * 100) : null,
  );

  /** The translated headline for a failure code; unknown codes get the general one. */
  function failText(code: string): string {
    const key = `fail.${code}`;
    return t(hasKey(key) ? (key as Key) : "fail.other");
  }

  let copied = $state(false);
  async function copyAddress() {
    if (!server) return;
    try {
      await navigator.clipboard.writeText(server.address);
      copied = true;
      setTimeout(() => (copied = false), 1500);
    } catch {
      // No clipboard access: nothing to do, the address is selectable text anyway.
    }
  }
</script>

<aside>
  {#if !server}
    <div class="placeholder dim">{t("panel.pick")}</div>
  {:else}
    {@const level = runLevelText(server.run_level)}
    {@const round = server.run_level === 1 ? roundDuration(server.round_start_time) : null}
    <div class="info">
      <h2>{serverTitle(server)}</h2>
      <div class="addr dim">
        <span class="addr-text">{server.address}</span>
        <button class="link" onclick={copyAddress}>
          {copied ? t("panel.copied") : t("panel.copyAddress")}
        </button>
      </div>

      <dl>
        <dt>{t("panel.players")}</dt>
        <dd>
          {server.players}{#if server.soft_max_players > 0} / {server.soft_max_players}{/if}
        </dd>
        {#if level}
          <dt>{t("panel.status")}</dt>
          <dd>{[level, round].filter(Boolean).join(" · ")}</dd>
        {/if}
        {#if server.map}
          <dt>{t("panel.map")}</dt>
          <dd>{server.map}</dd>
        {/if}
        {#if server.preset}
          <dt>{t("panel.preset")}</dt>
          <dd>{server.preset}</dd>
        {/if}
        {#if server.panic_bunker}
          <dt>{t("panel.bunker")}</dt>
          <dd class="warn">{t("panel.bunkerOn")}</dd>
        {/if}
        {#if played && played.play_seconds > 0}
          <dt>{t("panel.playedTime")}</dt>
          <dd>{formatPlayed(played.play_seconds)}</dd>
        {/if}
      </dl>

      {#if server.tags.length > 0}
        <div class="tags">
          {#each server.tags as tag}<span class="tag">{tag}</span>{/each}
        </div>
      {/if}

      {#if ready}
        {#if ready.description}
          <p class="desc">{ready.description}</p>
        {/if}
        {#if ready.links.length > 0}
          <div class="links">
            {#each ready.links as link (link.url)}
              <button class="link" onclick={() => api.openUrl(link.url)} title={link.url}>
                {link.name}
              </button>
            {/each}
          </div>
        {/if}
        {#if ready.auth === "required"}
          <div class="note warn small">{t("panel.authRequired")}</div>
        {:else if ready.auth === "disabled"}
          <div class="note dim small">{t("panel.authDisabled")}</div>
        {/if}
      {:else if details === "failed"}
        <div class="note dim small">{t("panel.infoFailed")}</div>
      {/if}
    </div>

    <div class="play">
      {#if app.fromLink}
        <div class="redial">{t("play.fromLink")}</div>
      {/if}

      {#if app.redial}
        <div class="redial">
          <b>{t("play.redial")}</b>
          {#if app.redial.reason}
            <div class="reason">{t("play.redialReason", { reason: app.redial.reason })}</div>
          {/if}
        </div>
      {/if}

      <label class="field">
        <span class="dim">{t("play.as")}</span>
        <select bind:value={app.selectedAccount} disabled={app.busy}>
          <option value={null}>{t("play.guest", { name: app.settings?.guest_name ?? "…" })}</option>
          {#each app.accounts as a (a.user_id)}
            <option value={a.user_id}>
              {a.username}{a.state === "expired" ? ` (${t("play.needLogin")})` : ""}
            </option>
          {/each}
        </select>
      </label>
      <button class="link" onclick={onmanageaccounts}>{t("play.accounts")}</button>

      {#if account?.state === "expired"}
        <div class="error small">{t("play.expired")}</div>
      {:else if !account && !needsAccount}
        <div class="dim small">{t("play.guestHint")}</div>
      {/if}

      {#if app.info?.loader_problem}
        <div class="error small">{app.info.loader_problem}</div>
      {/if}

      {#if progress}
        <div class="progress">
          <div class="label">
            <span>{stageText(progress.stage)}</span>
            {#if percent !== null}
              <span class="dim">
                {#if progress.unit === "bytes"}
                  {formatBytes(progress.done)} / {formatBytes(progress.total)}
                {:else}
                  {Math.round(percent)}%
                {/if}
              </span>
            {/if}
          </div>
          <div class="bar" class:indeterminate={percent === null}>
            <div class="fill" style:width={percent === null ? undefined : `${percent}%`}></div>
          </div>
        </div>
        {#if progress.stage !== "starting"}
          <button onclick={() => app.cancelPlay()}>{t("play.cancel")}</button>
        {/if}
      {:else if app.play.phase === "running"}
        <div class="running">{t("play.running", { name: app.play.playsAs })}</div>
      {:else}
        <button class="primary big" disabled={!playable} onclick={() => app.startPlay()}>
          {t("play.button")}
        </button>
      {/if}

      {#if app.play.phase === "failed"}
        <div class="result">
          <div class="error">{failText(app.play.code)}</div>
          {#if app.play.message}
            <div class="dim detail">{app.play.message}</div>
          {/if}
          <button class="link" onclick={() => app.dismissPlayResult()}>{t("play.hide")}</button>
        </div>
      {:else if app.play.phase === "exited"}
        <div class="result">
          <div class="error">
            {app.play.code !== null
              ? t("play.crashedCode", { code: app.play.code })
              : t("play.crashed")}
          </div>
          {#if app.play.logTail.length > 0}
            <pre>{app.play.logTail.join("\n")}</pre>
          {/if}
          <div class="row">
            <button class="link" onclick={() => app.dismissPlayResult()}>{t("play.hide")}</button>
            <button class="link" onclick={() => api.openLogsFolder()}>{t("play.openLogs")}</button>
          </div>
        </div>
      {/if}
    </div>
  {/if}
</aside>

<style>
  aside {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
    border-left: 1px solid var(--border);
    background: var(--bg-elev);
  }
  .placeholder {
    margin: auto;
  }
  .info {
    flex: 1;
    overflow-y: auto;
    padding: 16px;
  }
  h2 {
    margin: 0 0 2px;
    font-size: 17px;
    user-select: text;
  }
  .addr {
    display: flex;
    align-items: baseline;
    gap: 10px;
    font-size: 12px;
    margin-bottom: 14px;
  }
  .addr-text {
    flex: 1;
    min-width: 0;
    user-select: text;
    word-break: break-all;
  }
  dl {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 4px 12px;
    margin: 0 0 14px;
  }
  dt {
    color: var(--text-dim);
  }
  dd {
    margin: 0;
  }
  .warn {
    color: var(--warn);
  }
  .tags {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  .tag {
    font-size: 11px;
    padding: 1px 7px;
    border-radius: 10px;
    background: var(--bg);
    border: 1px solid var(--border);
    color: var(--text-dim);
  }
  .desc {
    margin: 14px 0 0;
    white-space: pre-wrap;
    word-break: break-word;
    user-select: text;
    font-size: 13px;
  }
  .links {
    display: flex;
    flex-wrap: wrap;
    gap: 6px 14px;
    margin-top: 12px;
  }
  .note {
    margin-top: 12px;
  }
  .play {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 16px;
    border-top: 1px solid var(--border);
  }
  .redial {
    padding: 8px 10px;
    border-radius: var(--radius);
    border: 1px solid var(--warn);
    font-size: 13px;
  }
  .reason {
    margin-top: 4px;
    white-space: pre-wrap;
    word-break: break-word;
    user-select: text;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .small {
    font-size: 12px;
  }
  .big {
    padding: 11px;
    font-size: 16px;
  }
  .link {
    background: none;
    border: none;
    padding: 0;
    color: var(--accent);
    align-self: flex-start;
  }
  .link:hover:not(:disabled) {
    background: none;
    text-decoration: underline;
  }
  .progress .label {
    display: flex;
    justify-content: space-between;
    margin-bottom: 4px;
  }
  .bar {
    height: 8px;
    border-radius: 4px;
    background: var(--bg);
    overflow: hidden;
    position: relative;
  }
  .fill {
    height: 100%;
    background: var(--accent);
    transition: width 0.15s;
  }
  .indeterminate .fill {
    position: absolute;
    width: 35%;
    animation: slide 1.1s infinite ease-in-out;
  }
  @keyframes slide {
    from {
      left: -35%;
    }
    to {
      left: 100%;
    }
  }
  .running {
    padding: 10px;
    border-radius: var(--radius);
    background: var(--bg);
    border: 1px solid var(--good);
  }
  .result {
    display: flex;
    flex-direction: column;
    gap: 6px;
    font-size: 13px;
    user-select: text;
  }
  .result .row {
    display: flex;
    gap: 14px;
  }
  .detail {
    font-size: 12px;
    word-break: break-word;
  }
  pre {
    margin: 0;
    max-height: 140px;
    overflow: auto;
    padding: 8px;
    background: var(--bg);
    border-radius: 6px;
    font-size: 11px;
    white-space: pre-wrap;
    word-break: break-all;
  }
</style>
