<script lang="ts">
  import { onMount } from "svelte";
  import {
    api,
    errorText,
    type DiagRow,
    type PackRow,
    type StorageRow,
    type VersionRow,
  } from "./api";
  import { formatBytes } from "./format";
  import { t, type Key } from "./i18n.svelte";
  import Option from "./Option.svelte";
  import { app } from "./store.svelte";
  import {
    DEFAULT_CUSTOM,
    THEME_IDS,
    exportCustom,
    importCustom,
    type CustomColors,
  } from "./themes";

  const s = $derived(app.settings);

  // ------------------------------------------------------------ Windows integration
  let autostart = $state(false);
  let links = $state(false);

  onMount(() => {
    void api.getAutostart().then((v) => (autostart = v)).catch(() => {});
    void api.getLinksRegistered().then((v) => (links = v)).catch(() => {});
    void loadPacks();
    void loadStorage();
  });

  async function setAutostart(on: boolean) {
    try {
      autostart = await api.setAutostart(on);
    } catch (e) {
      app.say(errorText(e));
      autostart = !on;
    }
  }

  async function setLinks(on: boolean) {
    try {
      links = await api.setLinksRegistered(on);
    } catch (e) {
      app.say(errorText(e));
      links = !on;
    }
  }

  // ------------------------------------------------------------ appearance
  const SYSTEM_FONT = "Segoe UI";
  const fontMode = $derived(!s || s.font === "" ? "default" : s.font === SYSTEM_FONT ? "system" : "custom");
  // The text field keeps what is typed even while the mode is "custom" with an empty name.
  let customFont = $state("");
  let customMode = $state(false);

  function chooseFont(mode: string) {
    customMode = mode === "custom";
    if (mode === "default") app.updateSettings({ font: "" });
    else if (mode === "system") app.updateSettings({ font: SYSTEM_FONT });
    else app.updateSettings({ font: customFont });
  }

  const custom = $derived(s?.custom_theme ?? DEFAULT_CUSTOM);
  const COLORS: [keyof CustomColors, Key][] = [
    ["background", "opt.custom.background"],
    ["accent", "opt.custom.accent"],
    ["foreground", "opt.custom.foreground"],
    ["popup", "opt.custom.popup"],
    ["gradient_start", "opt.custom.gradientStart"],
    ["gradient_end", "opt.custom.gradientEnd"],
  ];

  function setColor(key: keyof CustomColors, value: string) {
    // Only complete colours are taken; half-typed ones wait.
    if (/^#[0-9a-fA-F]{6}$/.test(value)) {
      app.updateSettings({ custom_theme: { ...custom, [key]: value.toUpperCase() } });
    }
  }

  let themeNote = $state<string | null>(null);
  let importText = $state("");

  function flashTheme(text: string) {
    themeNote = text;
    setTimeout(() => (themeNote = null), 3000);
  }

  async function copyTheme() {
    try {
      await navigator.clipboard.writeText(exportCustom(custom));
      flashTheme(t("opt.custom.copied"));
    } catch {
      flashTheme(exportCustom(custom));
    }
  }

  function applyImport() {
    const got = importCustom(importText, custom);
    if (!got) return flashTheme(t("opt.custom.importFailed"));
    app.updateSettings({ custom_theme: got });
    importText = "";
    flashTheme(t("opt.custom.imported"));
  }

  // ------------------------------------------------------------ server list
  const COLUMNS: ["round_time" | "players" | "map" | "mode" | "ping", Key][] = [
    ["round_time", "opt.col.roundTime"],
    ["players", "opt.col.players"],
    ["map", "opt.col.map"],
    ["mode", "opt.col.mode"],
    ["ping", "opt.col.ping"],
  ];

  function setColumn(key: (typeof COLUMNS)[number][0], on: boolean) {
    if (s) app.updateSettings({ columns: { ...s.columns, [key]: on } });
  }

  // ------------------------------------------------------------ text packs
  let packs = $state<PackRow[]>([]);

  async function loadPacks() {
    packs = await api.listPacks().catch(() => []);
  }

  /** Enabled packs first, in the order they are used; the others after. */
  const orderedPacks = $derived.by(() => {
    const enabled = s?.enabled_packs ?? [];
    const byId = new Map(packs.map((p) => [p.id, p]));
    const used = enabled.map((id) => byId.get(id)).filter((p): p is PackRow => !!p);
    return [...used.map((p) => ({ ...p, enabled: true })), ...packs.filter((p) => !enabled.includes(p.id)).map((p) => ({ ...p, enabled: false }))];
  });

  function togglePack(pack: PackRow, on: boolean) {
    if (!s) return;
    const rest = s.enabled_packs.filter((id) => id !== pack.id);
    app.updateSettings({ enabled_packs: on ? [...rest, pack.id] : rest });
  }

  function movePack(id: string, delta: number) {
    if (!s) return;
    const list = [...s.enabled_packs];
    const i = list.indexOf(id);
    const j = i + delta;
    if (i < 0 || j < 0 || j >= list.length) return;
    [list[i], list[j]] = [list[j], list[i]];
    app.updateSettings({ enabled_packs: list });
  }

  // ------------------------------------------------------------ data
  let storage = $state<StorageRow | null>(null);
  let cleaning = $state(false);
  let cleanResult = $state<string | null>(null);

  async function loadStorage() {
    storage = await api.storageInfo().catch(() => null);
  }

  async function clean() {
    cleaning = true;
    cleanResult = null;
    try {
      const r = await api.cleanStorage();
      cleanResult =
        r.freed > 0 ? t("opt.cleaned", { size: formatBytes(r.freed) }) : t("opt.cleanedNothing");
      await loadStorage();
    } catch (e) {
      cleanResult = errorText(e);
    } finally {
      cleaning = false;
    }
  }

  // ------------------------------------------------------------ hidden debug section
  const DEBUG_KEY = "bebra.debug";
  const CLICKS_TO_UNLOCK = 7;
  let debug = $state(false);
  let clicks = 0;
  let clickTimer: ReturnType<typeof setTimeout> | undefined;

  try {
    debug = localStorage.getItem(DEBUG_KEY) === "1";
  } catch {
    // No storage: debug is just off.
  }

  function setDebug(on: boolean) {
    debug = on;
    try {
      localStorage.setItem(DEBUG_KEY, on ? "1" : "0");
    } catch {
      // Not remembered; still works for this run.
    }
  }

  function versionClicked() {
    clicks += 1;
    clearTimeout(clickTimer);
    clickTimer = setTimeout(() => (clicks = 0), 2000);
    if (clicks >= CLICKS_TO_UNLOCK && !debug) {
      setDebug(true);
      app.say(t("dbg.unlocked"));
      clicks = 0;
    }
  }

  let proxyDraft = $state<string | null>(null);
  const proxyShown = $derived(proxyDraft ?? s?.proxy ?? "");
  const proxyBad = $derived(
    proxyShown.trim() !== "" && !/^(https?|socks5h?):\/\/\S+$/i.test(proxyShown.trim()),
  );

  function editProxy(value: string) {
    proxyDraft = value;
    if (!proxyBad || value.trim() === "") app.updateSettings({ proxy: value.trim() });
  }

  let previewAddress = $state("");
  let copiedPreview = $state(false);

  async function copyText(text: string, flag: "preview" | "diag") {
    try {
      await navigator.clipboard.writeText(text);
      if (flag === "preview") {
        copiedPreview = true;
        setTimeout(() => (copiedPreview = false), 1500);
      } else {
        copiedDiag = true;
        setTimeout(() => (copiedDiag = false), 1500);
      }
    } catch {
      // No clipboard access: the text is selectable.
    }
  }

  let dumpResult = $state<string | null>(null);
  async function dump() {
    dumpResult = null;
    try {
      const r = await api.dumpLocale();
      dumpResult = t("dbg.dumped", { files: r.files, size: formatBytes(r.bytes) });
    } catch (e) {
      dumpResult = errorText(e);
    }
  }

  let diag = $state<DiagRow[] | null>(null);
  let diagBusy = $state(false);
  let copiedDiag = $state(false);

  async function runDiag() {
    diagBusy = true;
    try {
      diag = await api.diagnostics();
    } catch (e) {
      diag = [{ name: "diagnostics", ok: false, ms: null, detail: errorText(e) }];
    } finally {
      diagBusy = false;
    }
  }

  const diagReport = $derived(
    [
      `BebraLauncher ${app.info?.version ?? ""}`,
      ...(diag ?? []).map(
        (r) => `${r.ok ? "OK  " : "FAIL"} ${r.name}${r.ms !== null ? ` (${r.ms} ms)` : ""}: ${r.detail}`,
      ),
    ].join("\n"),
  );

  let versions = $state<VersionRow[] | null>(null);
  async function loadVersions() {
    versions = await api.contentVersions().catch(() => []);
  }

  let confirmReset = $state(false);
  async function reset() {
    if (!confirmReset) {
      confirmReset = true;
      setTimeout(() => (confirmReset = false), 4000);
      return;
    }
    confirmReset = false;
    try {
      await api.resetSettings();
      await app.loadSettings();
    } catch (e) {
      app.say(errorText(e));
    }
  }
</script>

{#if s}
  <div class="tab">
    <!-- ------------------------------------------------------------ general -->
    <section>
      <h2>{t("opt.general")}</h2>

      <label class="field">
        <span>{t("opt.language")}</span>
        <select value={s.language} onchange={(e) => app.updateSettings({ language: e.currentTarget.value })}>
          <option value="auto">{t("opt.langAuto")}</option>
          <option value="ru">Русский</option>
          <option value="en">English</option>
        </select>
      </label>

      <div class="field">
        <button class="btn" onclick={() => api.openUrl("https://account.spacestation14.com/Identity/Account/Manage")}>
          {t("opt.accountSettings")}
        </button>
        <small class="sub">{t("opt.accountSettingsDesc")}</small>
      </div>

      <label class="field">
        <span>{t("opt.guestName")}</span>
        <input
          type="text"
          maxlength="32"
          value={s.guest_name}
          oninput={(e) => app.updateSettings({ guest_name: e.currentTarget.value })}
        />
        <small class="sub">{t("opt.guestHint")}</small>
      </label>

      <Option label={t("opt.compat")} hint={t("opt.compatHint")} checked={s.compat_mode} onchange={(v) => app.updateSettings({ compat_mode: v })} />
      <Option label={t("opt.minimize")} hint={t("opt.minimizeHint")} checked={s.minimize_on_play} onchange={(v) => app.updateSettings({ minimize_on_play: v })} />
      <Option label={t("opt.autostart")} hint={t("opt.autostartHint")} checked={autostart} onchange={setAutostart} />
      <Option label={t("opt.links")} hint={t("opt.linksHint")} checked={links} onchange={setLinks} />
      <Option label={t("opt.ignoreRedial")} hint={t("opt.ignoreRedialHint")} checked={s.ignore_redial} onchange={(v) => app.updateSettings({ ignore_redial: v })} />
    </section>

    <!-- ------------------------------------------------------------ appearance -->
    <section>
      <h2>{t("opt.appearance")}</h2>

      <label class="field">
        <span>{t("opt.theme")}</span>
        <select value={s.theme} onchange={(e) => app.updateSettings({ theme: e.currentTarget.value })}>
          {#each THEME_IDS as id}<option value={id}>{t(`theme.${id}` as Key)}</option>{/each}
        </select>
      </label>

      {#if s.theme === "custom"}
        <div class="custom">
          <h3>{t("opt.custom.title")}</h3>
          <div class="colors">
            {#each COLORS as [key, label]}
              <div class="color">
                <span>{t(label)}</span>
                <input type="color" value={custom[key]} oninput={(e) => setColor(key, e.currentTarget.value)} />
                <input type="text" class="hex" value={custom[key]} maxlength="7" spellcheck="false" oninput={(e) => setColor(key, e.currentTarget.value)} />
              </div>
            {/each}
          </div>
          <div class="row">
            <button class="btn" onclick={() => app.updateSettings({ custom_theme: { ...DEFAULT_CUSTOM } })}>{t("opt.custom.reset")}</button>
            <button class="btn" onclick={copyTheme}>{t("opt.custom.export")}</button>
            {#if themeNote}<span class="sub">{themeNote}</span>{/if}
          </div>
          <textarea rows="3" placeholder={t("opt.custom.importPlaceholder")} bind:value={importText} spellcheck="false"></textarea>
          <div><button class="btn" disabled={!importText.trim()} onclick={applyImport}>{t("opt.custom.import")}</button></div>
        </div>
      {/if}

      <Option label={t("opt.gradient")} checked={s.theme_gradient} onchange={(v) => app.updateSettings({ theme_gradient: v })} />
      <Option label={t("opt.decor")} checked={s.theme_decor} onchange={(v) => app.updateSettings({ theme_decor: v })} />

      <div class="field">
        <span>{t("opt.font")}</span>
        <select value={customMode ? "custom" : fontMode} onchange={(e) => chooseFont(e.currentTarget.value)}>
          <option value="default">{t("opt.fontDefault")}</option>
          <option value="system">{t("opt.fontSystem")}</option>
          <option value="custom">{t("opt.fontCustom")}</option>
        </select>
        {#if customMode || fontMode === "custom"}
          <input
            type="text"
            maxlength="64"
            placeholder="Consolas"
            value={s.font === SYSTEM_FONT ? "" : s.font}
            oninput={(e) => {
              customFont = e.currentTarget.value;
              app.updateSettings({ font: customFont });
            }}
          />
          <small class="sub">{t("opt.fontCustomHint")}</small>
        {/if}
      </div>
    </section>

    <!-- ------------------------------------------------------------ server list -->
    <section>
      <h2>{t("opt.list")}</h2>
      <div class="columns">
        {#each COLUMNS as [key, label]}
          <Option label={t(label)} checked={s.columns[key]} onchange={(v) => setColumn(key, v)} />
        {/each}
      </div>
    </section>

    <!-- ------------------------------------------------------------ text packs -->
    <section>
      <h2>{t("opt.packs")}</h2>
      <p class="sub">{t("opt.packsDesc")}</p>

      {#if orderedPacks.length === 0}
        <p class="sub">{t("opt.packsEmpty")}</p>
      {:else}
        <ul class="packs">
          {#each orderedPacks as pack, i (pack.id)}
            <li class:bad={pack.problems.length > 0}>
              <input
                type="checkbox"
                checked={pack.enabled}
                disabled={pack.problems.length > 0}
                onchange={(e) => togglePack(pack, e.currentTarget.checked)}
                aria-label={pack.name}
              />
              <div class="grow">
                <div>
                  <b>{pack.name}</b>
                  <span class="sub small">
                    {pack.id}{#if pack.problems.length === 0} · {t("pack.files", { count: pack.files })} · {pack.cultures.join(", ")}{/if}
                  </span>
                </div>
                {#if pack.description}<div class="sub small">{pack.description}</div>{/if}
                {#if pack.problems.length > 0}
                  <div class="error small">
                    {t("pack.invalid")}: {pack.problems.join("; ")}
                  </div>
                {/if}
              </div>
              {#if pack.enabled}
                <button class="btn icon open-r" title={t("pack.up")} aria-label={t("pack.up")} disabled={i === 0} onclick={() => movePack(pack.id, -1)}>▲</button>
                <button class="btn icon open-l" title={t("pack.down")} aria-label={t("pack.down")} disabled={i >= (s.enabled_packs.length - 1)} onclick={() => movePack(pack.id, 1)}>▼</button>
              {/if}
            </li>
          {/each}
        </ul>
        <p class="sub small">{t("opt.packsOrder")}</p>
      {/if}
      <div class="row">
        <button class="btn open-r" onclick={() => api.openPacksFolder()}>{t("opt.packsOpen")}</button>
        <button class="btn open-l" onclick={loadPacks}>{t("opt.packsRefresh")}</button>
      </div>
    </section>

    <!-- ------------------------------------------------------------ data -->
    <section>
      <h2>{t("opt.storage")}</h2>
      {#if storage}
        <div>{t("opt.storageTotal", { size: formatBytes(storage.total) })}</div>
        <small class="sub">
          {t("opt.storageDetails", {
            content: formatBytes(storage.content),
            engines: formatBytes(storage.engines),
            modules: formatBytes(storage.modules),
            other: formatBytes(storage.other),
          })}
        </small>
      {/if}
      <div class="row">
        <button class="btn" onclick={clean} disabled={cleaning || app.busy}>
          {cleaning ? t("opt.cleaning") : t("opt.clean")}
        </button>
        <button class="btn" onclick={() => api.openLogsFolder()}>{t("opt.openLogs")}</button>
        {#if cleanResult}<span class="sub">{cleanResult}</span>{/if}
      </div>
    </section>

    <!-- ------------------------------------------------------------ about -->
    <section>
      <h2>{t("opt.about")}</h2>
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <div class="version" onclick={versionClicked} title={debug ? "" : t("opt.hiddenHint")}>
        BebraLauncher {app.info?.version ?? ""}
      </div>
      <div class="sub small path">{t("opt.data", { path: app.info?.data_dir ?? "" })}</div>
      <div class="sub small">{t("opt.credits")}</div>
    </section>

    <!-- ------------------------------------------------------------ debug (hidden) -->
    {#if debug}
      <section class="debug">
        <h2>{t("dbg.title")}</h2>

        <Option label={t("dbg.verbose")} hint={t("dbg.verboseHint")} checked={s.verbose_log} onchange={(v) => app.updateSettings({ verbose_log: v })} />

        <label class="field">
          <span>{t("dbg.proxy")}</span>
          <input
            type="text"
            placeholder="socks5://127.0.0.1:1080"
            value={proxyShown}
            spellcheck="false"
            oninput={(e) => editProxy(e.currentTarget.value)}
          />
          <small class="sub">{t("dbg.proxyHint")}</small>
          {#if proxyBad}<small class="error">{t("dbg.proxyInvalid")}</small>{/if}
        </label>

        <div class="field">
          <span>{t("dbg.preview")}</span>
          <div class="row">
            <input type="text" class="grow" placeholder={t("direct.placeholder")} bind:value={previewAddress} spellcheck="false" />
            <button class="btn" disabled={!previewAddress.trim() || app.busy} onclick={() => app.launchPreview(previewAddress)}>
              {t("dbg.previewRun")}
            </button>
          </div>
          <small class="sub">{t("dbg.previewHint")}</small>
          {#if app.preview}
            <pre>{app.preview}</pre>
            <div><button class="link" onclick={() => copyText(app.preview ?? "", "preview")}>{copiedPreview ? t("dbg.copied") : t("dbg.copy")}</button></div>
          {/if}
        </div>

        <div class="field">
          <span>{t("dbg.dump")}</span>
          <div class="row">
            <button class="btn" onclick={dump}>{t("dbg.dumpRun")}</button>
            {#if dumpResult}<span class="sub">{dumpResult}</span>{/if}
          </div>
          <small class="sub">{t("dbg.dumpHint")}</small>
        </div>

        <div class="field">
          <span>{t("dbg.diag")}</span>
          <div class="row">
            <button class="btn" disabled={diagBusy} onclick={runDiag}>{diagBusy ? t("dbg.diagRunning") : t("dbg.diagRun")}</button>
            {#if diag}<button class="link" onclick={() => copyText(diagReport, "diag")}>{copiedDiag ? t("dbg.copied") : t("dbg.diagCopy")}</button>{/if}
          </div>
          {#if diag}
            <ul class="diag">
              {#each diag as r}
                <li>
                  <span class={r.ok ? "good" : "error"}>{r.ok ? "●" : "✕"}</span>
                  <b>{r.name}</b>
                  {#if r.ms !== null}<span class="sub">{r.ms} ms</span>{/if}
                  <span class="sub detail">{r.detail}</span>
                </li>
              {/each}
            </ul>
          {/if}
        </div>

        <div class="field">
          <span>{t("dbg.versions")}</span>
          <div><button class="btn" onclick={loadVersions}>{t("dbg.versionsRun")}</button></div>
          {#if versions}
            {#if versions.length === 0}
              <small class="sub">{t("dbg.versionsEmpty")}</small>
            {:else}
              <ul class="diag">
                {#each versions as v (v.id)}
                  <li>
                    <span class="sub detail">
                      {t("dbg.versionLine", { id: v.id, version: v.version || v.fork || "?", engine: v.engine || "?", files: v.files })}
                    </span>
                  </li>
                {/each}
              </ul>
            {/if}
          {/if}
        </div>

        <div class="row">
          <button class="btn" onclick={() => api.openDataFolder()}>{t("dbg.dataFolder")}</button>
          <button class="btn" onclick={reset}>{confirmReset ? t("dbg.resetConfirm") : t("dbg.reset")}</button>
          <button class="link" onclick={() => setDebug(false)}>{t("dbg.hide")}</button>
        </div>
        <small class="sub">{t("dbg.resetHint")}</small>
      </section>
    {/if}
  </div>
{/if}

<style>
  .tab {
    height: 100%;
    overflow-y: auto;
    padding: 8px 18px 24px;
    display: flex;
    flex-direction: column;
    gap: 22px;
  }
  section {
    display: flex;
    flex-direction: column;
    gap: 10px;
    max-width: 760px;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 4px;
    align-items: flex-start;
  }
  .field input[type="text"],
  .field select {
    min-width: 260px;
  }
  small {
    font-size: 13px;
  }
  .small {
    font-size: 13px;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-wrap: wrap;
  }
  .grow {
    flex: 1;
    min-width: 0;
  }
  p {
    margin: 0;
  }
  .columns {
    display: flex;
    flex-wrap: wrap;
    gap: 8px 22px;
  }
  .custom {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 10px 12px;
    border: 1px solid var(--ctrl-mid);
    background: var(--popup);
  }
  .colors {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(300px, 1fr));
    gap: 6px 18px;
  }
  .color {
    display: grid;
    grid-template-columns: 1fr auto 92px;
    align-items: center;
    gap: 8px;
  }
  .hex {
    width: 92px;
    min-width: 0 !important;
    font-family: "Cascadia Mono", Consolas, monospace;
  }
  textarea {
    width: 100%;
    resize: vertical;
    font-family: "Cascadia Mono", Consolas, monospace;
    font-size: 13px;
  }
  .packs {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .packs li {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 6px 8px;
    background: var(--popup);
    border: 1px solid var(--ctrl-mid);
  }
  .packs li.bad {
    border-color: var(--bad);
  }
  .version {
    cursor: default;
    width: max-content;
  }
  .path {
    user-select: text;
    word-break: break-all;
  }
  .debug {
    padding-top: 14px;
    border-top: 2px solid var(--accent);
  }
  pre {
    margin: 4px 0 0;
    padding: 8px;
    max-width: 100%;
    background: var(--popup);
    border: 1px solid var(--ctrl-mid);
    font-size: 12px;
    white-space: pre-wrap;
    word-break: break-all;
    user-select: text;
  }
  .diag {
    list-style: none;
    margin: 4px 0 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 3px;
    font-size: 14px;
  }
  .diag li {
    display: flex;
    gap: 8px;
    align-items: baseline;
    flex-wrap: wrap;
  }
  .detail {
    user-select: text;
    word-break: break-all;
  }
</style>
