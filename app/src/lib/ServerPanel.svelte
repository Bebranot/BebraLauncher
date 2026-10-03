<script lang="ts">
  import { api } from "./api";
  import { app } from "./store.svelte";
  import { formatBytes, roundDuration, runLevelText, serverTitle, stageText } from "./format";

  let { onmanageaccounts }: { onmanageaccounts: () => void } = $props();

  const server = $derived(app.selected);
  const account = $derived(app.accounts.find((a) => a.user_id === app.selectedAccount) ?? null);
  const playable = $derived(
    !!server && !app.busy && !app.info?.loader_problem && account?.state !== "expired",
  );

  const progress = $derived(app.play.phase === "preparing" ? app.play : null);
  const percent = $derived(
    progress && progress.total > 0 ? Math.min(100, (progress.done / progress.total) * 100) : null,
  );
</script>

<aside>
  {#if !server}
    <div class="placeholder dim">Выберите сервер в списке</div>
  {:else}
    {@const level = runLevelText(server.run_level)}
    {@const round = server.run_level === 1 ? roundDuration(server.round_start_time) : null}
    <div class="info">
      <h2>{serverTitle(server)}</h2>
      <div class="addr dim">{server.address}</div>

      <dl>
        <dt>Игроки</dt>
        <dd>
          {server.players}{#if server.soft_max_players > 0} / {server.soft_max_players}{/if}
        </dd>
        {#if level}
          <dt>Статус</dt>
          <dd>{[level, round].filter(Boolean).join(" · ")}</dd>
        {/if}
        {#if server.map}
          <dt>Карта</dt>
          <dd>{server.map}</dd>
        {/if}
        {#if server.preset}
          <dt>Режим</dt>
          <dd>{server.preset}</dd>
        {/if}
        {#if server.panic_bunker}
          <dt>Паник-бункер</dt>
          <dd class="warn">включён</dd>
        {/if}
      </dl>

      {#if server.tags.length > 0}
        <div class="tags">
          {#each server.tags as tag}<span class="tag">{tag}</span>{/each}
        </div>
      {/if}
    </div>

    <div class="play">
      <label class="field">
        <span class="dim">Играть как</span>
        <select bind:value={app.selectedAccount} disabled={app.busy}>
          <option value={null}>Гость ({app.settings?.guest_name ?? "…"})</option>
          {#each app.accounts as a (a.user_id)}
            <option value={a.user_id}>
              {a.username}{a.state === "expired" ? " (нужен вход)" : ""}
            </option>
          {/each}
        </select>
      </label>
      <button class="link" onclick={onmanageaccounts}>Аккаунты…</button>

      {#if account?.state === "expired"}
        <div class="error small">
          Вход в аккаунт истёк. Войдите заново в «Аккаунты».
        </div>
      {:else if !account}
        <div class="dim small">
          Гостем можно играть только на серверах без обязательной авторизации.
        </div>
      {/if}

      {#if app.info?.loader_problem}
        <div class="error small">{app.info.loader_problem}</div>
      {/if}

      {#if progress}
        <div class="progress">
          <div class="label">
            <span>{stageText[progress.stage]}</span>
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
          <button onclick={() => app.cancelPlay()}>Отмена</button>
        {/if}
      {:else if app.play.phase === "running"}
        <div class="running">Игра запущена как <b>{app.play.playsAs}</b></div>
      {:else}
        <button class="primary big" disabled={!playable} onclick={() => app.startPlay()}>
          Играть
        </button>
      {/if}

      {#if app.play.phase === "failed"}
        <div class="result error">
          <div>{app.play.message}</div>
          <button class="link" onclick={() => app.dismissPlayResult()}>Скрыть</button>
        </div>
      {:else if app.play.phase === "exited"}
        <div class="result">
          <div class="error">
            Игра завершилась с ошибкой{app.play.code !== null ? ` (код ${app.play.code})` : ""}.
          </div>
          {#if app.play.logTail.length > 0}
            <pre>{app.play.logTail.join("\n")}</pre>
          {/if}
          <div class="row">
            <button class="link" onclick={() => app.dismissPlayResult()}>Скрыть</button>
            <button class="link" onclick={() => api.openLogsFolder()}>
              Открыть папку логов
            </button>
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
    font-size: 12px;
    margin-bottom: 14px;
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
  .play {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 16px;
    border-top: 1px solid var(--border);
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
