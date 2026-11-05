<script lang="ts">
  import { onMount } from "svelte";
  import { api, errorText } from "./api";
  import { inline, parseChangelog, parseGroups, type ChangeEntry, type ChangeGroup } from "./changelog";
  import { formatBytes, formatDate, formatSpeed, formatTime } from "./format";
  import { t } from "./i18n.svelte";
  import Icon from "./Icon.svelte";
  import Option from "./Option.svelte";
  import { SpeedMeter } from "./speed";
  import { app } from "./store.svelte";

  const s = $derived(app.settings);
  const u = $derived(app.update);
  const phase = $derived(u?.phase ?? "idle");
  const latest = $derived(u?.latest ?? null);
  const current = $derived(u?.current ?? app.info?.version ?? "");
  const percent = $derived(
    u && u.phase === "downloading" && u.total > 0 ? Math.min(100, (u.done / u.total) * 100) : null,
  );
  const notes = $derived<ChangeGroup[]>(latest ? parseGroups(latest.notes.split(/\r?\n/)) : []);

  /** The one thing the top of the tab says right now. */
  const status = $derived.by(() => {
    if (phase === "downloading" && latest) return "downloading";
    if (phase === "ready" && latest) return "ready";
    if (phase === "checking") return "checking";
    if (latest) return "available";
    return u?.error ? "failed" : "current";
  });

  // Speed, in bits per second, from the downloaded byte count; a new phase starts the meter over.
  const meter = new SpeedMeter();
  let speed = $state(0);
  let lastPhase = "";
  $effect(() => {
    const st = u;
    const now = st?.phase ?? "idle";
    if (now !== lastPhase) {
      lastPhase = now;
      meter.reset();
    }
    if (st && now === "downloading") speed = meter.sample(st.done);
    else speed = 0;
  });
  $effect(() => {
    if (phase !== "downloading") return;
    const timer = setInterval(() => (speed = meter.rate()), 1000);
    return () => clearInterval(timer);
  });

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
</script>

{#if s}
  <div class="tab">
    <!-- ------------------------------------------------------------ status -->
    <section class="status {status}" aria-label={t("upd.title")} aria-live="polite">
      <div class="line">
        <span class="glyph">
          {#if status === "checking"}
            <span class="spinner"></span>
          {:else if status === "failed"}
            <Icon name="alert" size={20} />
          {:else if status === "available" || status === "downloading"}
            <Icon name="download" size={20} />
          {:else}
            <Icon name="check" size={20} />
          {/if}
        </span>

        <div class="text">
          {#if status === "downloading" && latest}
            <h2>{t("upd.downloading", { version: latest.version })}</h2>
            <p>
              {#if percent !== null && u}
                {formatBytes(u.done)} / {formatBytes(u.total)} · {Math.round(percent)}%{#if speed > 0} · {formatSpeed(speed)}{/if}
              {/if}
            </p>
          {:else if status === "ready" && latest}
            <h2>{t("upd.ready", { version: latest.version })}</h2>
            <p>
              {#if !u?.can_install}
                {app.play.phase !== "idle" ? t("upd.cannotPlaying") : t("upd.cannotDev")}
              {:else if s.update_auto}
                {t("upd.installOnExit")}
              {:else}
                {t("upd.installManual")}
              {/if}
            </p>
          {:else if status === "available" && latest}
            <h2>{t("upd.available", { version: latest.version })}</h2>
            <p>{t("upd.current", { version: current })} · {t("upd.size", { size: formatBytes(latest.size) })}</p>
          {:else if status === "checking"}
            <h2>{t("upd.checking")}</h2>
            <p>{t("upd.current", { version: current })}</p>
          {:else if status === "failed"}
            <h2>{t("upd.failed")}</h2>
            <p>{t("upd.current", { version: current })}</p>
          {:else}
            <h2>{t("upd.upToDate")}</h2>
            <p>
              {t("upd.current", { version: current })}{#if app.updateCheckedAt} · {t("upd.checkedAt", { time: formatTime(app.updateCheckedAt) })}{/if}
            </p>
          {/if}
        </div>

        <div class="actions">
          {#if status === "ready"}
            <button class="btn primary" disabled={!u?.can_install} onclick={() => app.installUpdate(true)}>
              {t("upd.installNow")}
            </button>
          {:else if status === "available"}
            <button class="btn primary" onclick={() => app.downloadUpdate()}>{t("upd.download")}</button>
          {:else if status !== "downloading"}
            <button class="btn" disabled={status === "checking"} onclick={() => app.checkUpdate()}>
              {t("upd.check")}
            </button>
          {/if}
        </div>
      </div>

      {#if status === "downloading"}
        <progress max="100" value={percent ?? undefined}></progress>
      {/if}
      {#if u?.error}<p class="problem">{t("upd.error", { error: u.error })}</p>{/if}
    </section>

    <div class="columns" data-scroll>
      <!-- ------------------------------------------------------------ what changed -->
      <div class="main">
        {#if latest && notes.length > 0}
          <section>
            <h2 class="heading">{t("upd.notes", { version: latest.version })}</h2>
            {#each notes as group}
              {@render groupView(group)}
            {/each}
          </section>
        {/if}

        <section>
          <h2 class="heading">{t("upd.history")}</h2>
          {#each history as entry, i (entry.version ?? "unreleased")}
            <details open={i === 0}>
              <summary>
                <span class="chevron"><Icon name="chevron" size={16} /></span>
                <b class="version">{entry.version ?? t("upd.unreleased")}</b>
                {#if entry.version && entry.version === current}
                  <span class="installed">{t("upd.installed")}</span>
                {/if}
                {#if entry.date}<span class="sub date">{formatDate(`${entry.date}T12:00:00`)}</span>{/if}
              </summary>
              <div class="changes">
                {#each entry.groups as group}
                  {@render groupView(group)}
                {/each}
              </div>
            </details>
          {:else}
            <p class="sub">{t("upd.historyEmpty")}</p>
          {/each}
        </section>
      </div>

      <!-- ------------------------------------------------------------ settings -->
      <aside>
        <h2 class="heading">{t("upd.settings")}</h2>
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

        <p class="signed sub">
          <Icon name="shield" size={16} />
          <span>{t("upd.signed")}</span>
        </p>

        <details class="source">
          <summary>
            <span class="chevron"><Icon name="chevron" size={16} /></span>
            {t("upd.source")}
          </summary>
          <div class="fields">
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
              <label for="update-token">{t("upd.token")}</label>
              <input
                id="update-token"
                type="password"
                autocomplete="off"
                spellcheck="false"
                placeholder={tokenSet ? "••••••••••••" : t("upd.tokenPlaceholder")}
                bind:value={token}
              />
              <div class="row">
                <button class="btn" disabled={token.trim() === ""} onclick={() => saveToken(token)}>{t("upd.tokenSave")}</button>
                {#if tokenSet}
                  <button class="btn ghost" onclick={() => saveToken("")}>{t("upd.tokenRemove")}</button>
                {/if}
              </div>
              <small class={tokenSet ? "good" : "sub"}>{tokenSet ? t("upd.tokenSaved") : t("upd.tokenNone")}</small>
              {#if tokenError}<small class="error">{tokenError}</small>{/if}
              <small class="sub">{t("upd.tokenHint")}</small>
            </div>
          </div>
        </details>
      </aside>
    </div>
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
    display: flex;
    flex-direction: column;
    min-height: 0;
  }

  /* ---- status: the tone of the whole band says how things stand */
  .status {
    flex: none;
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 14px 18px;
    background: var(--tint);
    transition: background 0.2s ease;
  }
  .status.available,
  .status.downloading {
    background: var(--accent-soft);
  }
  .status.ready {
    background: color-mix(in srgb, var(--good) 16%, transparent);
  }
  .status.failed {
    background: color-mix(in srgb, var(--bad) 14%, transparent);
  }
  .line {
    display: flex;
    align-items: center;
    gap: 14px;
  }
  .glyph {
    flex: none;
    display: grid;
    place-items: center;
    width: 36px;
    height: 36px;
    border-radius: 50%;
    background: var(--tint-hover);
    color: var(--sub);
  }
  .current .glyph,
  .ready .glyph {
    color: var(--good);
  }
  .available .glyph,
  .downloading .glyph {
    color: var(--accent);
  }
  .failed .glyph {
    color: var(--bad);
  }
  .text {
    flex: 1;
    min-width: 0;
  }
  .text h2 {
    font-size: 16px;
    line-height: 1.3;
  }
  .text p {
    margin: 2px 0 0;
    font-size: 13px;
    color: color-mix(in srgb, var(--fg) 72%, transparent);
    font-variant-numeric: tabular-nums;
  }
  .actions {
    flex: none;
  }
  .status progress {
    width: 100%;
    display: block;
  }
  .problem {
    margin: 0;
    font-size: 13px;
    color: var(--fg);
    overflow-wrap: anywhere;
    user-select: text;
  }

  /* ---- what changed on the left, settings on the right */
  .columns {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    display: grid;
    grid-template-columns: minmax(0, 1fr) 280px;
    align-items: start;
    gap: 28px;
    padding: 18px 18px 28px;
  }
  .main {
    display: flex;
    flex-direction: column;
    gap: 24px;
    min-width: 0;
  }
  .heading {
    margin-bottom: 8px;
    font-size: 14px;
  }
  aside {
    display: flex;
    flex-direction: column;
    gap: 14px;
    padding: 14px;
    border-radius: 12px;
    background: var(--tint);
  }
  aside .heading {
    margin-bottom: 0;
  }
  .signed {
    display: flex;
    gap: 8px;
    margin: 0;
    font-size: 13px;
    line-height: 1.35;
  }
  .signed :global(svg) {
    margin-top: 1px;
  }
  .fields {
    display: flex;
    flex-direction: column;
    gap: 14px;
    padding-top: 10px;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 5px;
    align-items: stretch;
  }
  .field input {
    width: 100%;
    box-sizing: border-box;
  }
  .row {
    display: flex;
    gap: 6px;
    flex-wrap: wrap;
  }
  small {
    font-size: 13px;
    line-height: 1.35;
  }

  /* ---- the changelog */
  details {
    border-top: 1px solid var(--line);
  }
  aside details {
    border-top: 0;
  }
  summary {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 9px 2px;
    cursor: pointer;
    list-style: none;
    border-radius: var(--radius-sm);
  }
  aside summary {
    padding: 2px 0;
    color: var(--sub);
    font-size: 13px;
  }
  summary:hover {
    color: var(--fg);
  }
  summary::-webkit-details-marker {
    display: none;
  }
  .chevron {
    color: var(--muted);
    transform: rotate(-90deg);
    transition: transform 0.15s ease;
  }
  details[open] > summary .chevron {
    transform: none;
  }
  .version {
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }
  .installed {
    padding: 1px 7px;
    border-radius: 999px;
    background: var(--accent-soft);
    font-size: 12px;
  }
  .date {
    margin-left: auto;
    font-size: 13px;
    font-variant-numeric: tabular-nums;
  }
  .changes {
    padding: 0 2px 12px 24px;
  }
  .group h3 {
    margin: 10px 0 4px;
    font-size: 13px;
    color: var(--sub);
  }
  .changes .group:first-child h3 {
    margin-top: 0;
  }
  ul {
    margin: 0;
    padding-left: 16px;
    display: flex;
    flex-direction: column;
    gap: 4px;
    max-width: 75ch;
  }
  li {
    line-height: 1.45;
    user-select: text;
  }
  code {
    padding: 0 5px;
    border-radius: 4px;
    background: var(--tint-hover);
    font-family: "Cascadia Mono", Consolas, monospace;
    font-size: 12.5px;
  }

  @media (max-width: 760px) {
    .columns {
      grid-template-columns: minmax(0, 1fr);
    }
    aside {
      order: -1;
    }
  }
</style>
