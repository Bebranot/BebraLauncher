<script lang="ts">
  import { onMount } from "svelte";
  import { api, errorText } from "./api";
  import { inline, parseChangelog, parseGroups, type ChangeEntry, type ChangeGroup } from "./changelog";
  import { formatBytes } from "./format";
  import { t } from "./i18n.svelte";
  import Option from "./Option.svelte";
  import { app } from "./store.svelte";

  const s = $derived(app.settings);
  const u = $derived(app.update);
  const phase = $derived(u?.phase ?? "idle");
  const latest = $derived(u?.latest ?? null);
  const percent = $derived(
    u && u.phase === "downloading" && u.total > 0 ? Math.min(100, (u.done / u.total) * 100) : null,
  );
  const notes = $derived<ChangeGroup[]>(latest ? parseGroups(latest.notes.split(/\r?\n/)) : []);

  // ------------------------------------------------------------ changelog
  let history = $state<ChangeEntry[]>([]);
  onMount(() => {
    void api
      .changelog()
      .then((text) => (history = parseChangelog(text)))
      .catch(() => {});
    void api.updateTokenState().then((v) => (tokenSet = v)).catch(() => {});
  });

  // ------------------------------------------------------------ repository and token
  let tokenSet = $state(false);
  let token = $state("");
  let tokenError = $state<string | null>(null);

  async function saveToken(value: string) {
    tokenError = null;
    try {
      tokenSet = await api.setUpdateToken(value);
      token = "";
    } catch (e) {
      tokenError = errorText(e);
    }
  }

  function time(ms: number): string {
    return new Date(ms).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  }
</script>

{#if s}
  <div class="tab">
    <!-- ------------------------------------------------------------ status -->
    <section class="card">
      <div class="head">
        <div>
          <h2>{t("upd.title")}</h2>
          <div class="sub">{t("upd.current", { version: u?.current ?? app.info?.version ?? "" })}</div>
        </div>
        <button class="btn" disabled={phase === "checking" || phase === "downloading"} onclick={() => app.checkUpdate()}>
          {#if phase === "checking"}<span class="spinner small"></span>{t("upd.checking")}{:else}{t("upd.check")}{/if}
        </button>
      </div>

      {#if phase === "downloading" && latest}
        <div class="state">
          <b>{t("upd.downloading", { version: latest.version })}</b>
          <progress max="100" value={percent ?? undefined}></progress>
          {#if percent !== null && u}
            <span class="sub small">{formatBytes(u.done)} / {formatBytes(u.total)} · {Math.round(percent)}%</span>
          {/if}
        </div>
      {:else if phase === "ready" && latest}
        <div class="state">
          <b class="good">{t("upd.ready", { version: latest.version })}</b>
          <div class="row">
            <button class="btn primary" disabled={!u?.can_install} onclick={() => app.installUpdate(true)}>
              {t("upd.installNow")}
            </button>
          </div>
          <span class="sub small">
            {#if !u?.can_install}
              {app.play.phase !== "idle" ? t("upd.cannotPlaying") : t("upd.cannotDev")}
            {:else if s.update_auto}
              {t("upd.installOnExit")}
            {:else}
              {t("upd.installManual")}
            {/if}
          </span>
        </div>
      {:else if latest}
        <div class="state">
          <b class="accent">{t("upd.available", { version: latest.version })}</b>
          <span class="sub small">{t("upd.size", { size: formatBytes(latest.size) })}</span>
          <div class="row">
            <button class="btn primary" onclick={() => app.downloadUpdate()}>{t("upd.download")}</button>
          </div>
        </div>
      {:else if phase === "idle"}
        <div class="state">
          {#if !u?.error}<span>{t("upd.upToDate")}</span>{/if}
          {#if app.updateCheckedAt}<span class="sub small">{t("upd.checkedAt", { time: time(app.updateCheckedAt) })}</span>{/if}
        </div>
      {/if}

      {#if u?.error}<p class="error">{t("upd.error", { error: u.error })}</p>{/if}
      <p class="sub small hint">{t("upd.signed")}</p>
    </section>

    <!-- ------------------------------------------------------------ release notes -->
    {#if latest && notes.length > 0}
      <section class="card">
        <h2 class="eyebrow">{t("upd.notes", { version: latest.version })}</h2>
        {#each notes as group}
          {@render groupView(group)}
        {/each}
      </section>
    {/if}

    <!-- ------------------------------------------------------------ settings -->
    <section class="card">
      <h2 class="eyebrow">{t("upd.settings")}</h2>
      <Option
        label={t("upd.optCheck")}
        hint={t("upd.optCheckHint")}
        checked={s.update_check}
        onchange={(v) => app.updateSettings({ update_check: v })}
      />
      <Option
        label={t("upd.optAuto")}
        hint={t("upd.optAutoHint")}
        checked={s.update_auto}
        onchange={(v) => app.updateSettings({ update_auto: v })}
      />

      <label class="field">
        <span>{t("upd.repo")}</span>
        <input
          type="text"
          spellcheck="false"
          value={s.update_repo}
          onchange={(e) => app.updateSettings({ update_repo: e.currentTarget.value })}
        />
        <small class="sub">{t("upd.repoHint")}</small>
      </label>

      <div class="field">
        <span>{t("upd.token")}</span>
        <div class="row">
          <input
            type="password"
            autocomplete="off"
            spellcheck="false"
            placeholder={tokenSet ? "••••••••••••" : t("upd.tokenPlaceholder")}
            bind:value={token}
          />
          <button class="btn" disabled={token.trim() === ""} onclick={() => saveToken(token)}>{t("upd.tokenSave")}</button>
          {#if tokenSet}
            <button class="btn ghost" onclick={() => saveToken("")}>{t("upd.tokenRemove")}</button>
          {/if}
        </div>
        <small class={tokenSet ? "good" : "sub"}>{tokenSet ? t("upd.tokenSaved") : t("upd.tokenNone")}</small>
        {#if tokenError}<small class="error">{tokenError}</small>{/if}
        <small class="sub">{t("upd.tokenHint")}</small>
      </div>
    </section>

    <!-- ------------------------------------------------------------ changelog -->
    <section class="card">
      <h2 class="eyebrow">{t("upd.history")}</h2>
      {#each history as entry, i (entry.version ?? "unreleased")}
        <details open={i === 0}>
          <summary>
            <b class:accent={entry.version === u?.current}>{entry.version ?? "…"}</b>
            {#if entry.date}<span class="sub">{entry.date}</span>{/if}
          </summary>
          {#each entry.groups as group}
            {@render groupView(group)}
          {/each}
        </details>
      {:else}
        <p class="sub">{t("upd.historyEmpty")}</p>
      {/each}
    </section>
  </div>
{/if}

{#snippet groupView(group: ChangeGroup)}
  <div class="group">
    {#if group.title}<h3>{group.title}</h3>{/if}
    <ul>
      {#each group.items as item}
        <li>
          {#each inline(item) as piece}{#if piece.code}<code>{piece.text}</code>{:else}{piece.text}{/if}{/each}
        </li>
      {/each}
    </ul>
  </div>
{/snippet}

<style>
  .tab {
    height: 100%;
    overflow-y: auto;
    padding: 16px 14px 28px;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 14px;
  }
  .head {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 12px;
  }
  .state {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 6px;
  }
  .state progress {
    width: 100%;
  }
  .accent {
    color: var(--accent);
  }
  .small {
    font-size: 13px;
  }
  .hint {
    margin: 0;
  }
  p {
    margin: 0;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-wrap: wrap;
  }
  .spinner.small {
    width: 14px;
    height: 14px;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 4px;
    align-items: flex-start;
  }
  .field input {
    min-width: 280px;
  }
  small {
    font-size: 13px;
  }
  .group h3 {
    margin: 8px 0 2px;
    font-size: 13px;
    color: var(--sub);
  }
  ul {
    margin: 0;
    padding-left: 18px;
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  li {
    user-select: text;
  }
  code {
    padding: 0 5px;
    border-radius: 4px;
    background: var(--tint-hover);
    font-family: "Cascadia Mono", Consolas, monospace;
    font-size: 12.5px;
  }
  details {
    border-top: 1px solid var(--line);
    padding: 8px 0 4px;
  }
  details:first-of-type {
    border-top: 0;
    padding-top: 0;
  }
  summary {
    display: flex;
    align-items: baseline;
    gap: 10px;
    cursor: pointer;
    list-style: none;
    padding: 2px 0;
  }
  summary::-webkit-details-marker {
    display: none;
  }
  summary::before {
    content: "›";
    color: var(--muted);
    transition: transform 0.12s;
  }
  details[open] > summary::before {
    transform: rotate(90deg);
  }
</style>
