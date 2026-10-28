<script lang="ts">
  import { onMount } from "svelte";
  import {
    api,
    errorText,
    type DiagRow,
    type PackRow,
    type PatchRow,
    type RecommendedHub,
    type StorageRow,
    type VersionRow,
  } from "./api";
  import { formatBytes } from "./format";
  import { t, type Key } from "./i18n.svelte";
  import { nativeName, UI_LANGUAGES } from "./languages";
  import ErpPromo from "./ErpPromo.svelte";
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

  let recommendedHubs = $state<RecommendedHub[]>([]);

  onMount(() => {
    void api.getAutostart().then((v) => (autostart = v)).catch(() => {});
    void api.getLinksRegistered().then((v) => (links = v)).catch(() => {});
    void api.getRecommendedHubs().then((r) => (recommendedHubs = r)).catch(() => {});
    void loadPacks();
    void loadPatches();
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

  // ------------------------------------------------------------ backgrounds
  let bgFileInput = $state<HTMLInputElement>();
  let uploadingBg = $state(false);
  let bgThumbnails = $state<Record<string, string>>({});

  $effect(() => {
    const list = app.backgrounds;
    for (const bg of list) {
      if (!bgThumbnails[bg.id]) {
        void api.getBackground(bg.id).then((url) => {
          bgThumbnails = { ...bgThumbnails, [bg.id]: url };
        }).catch(() => {});
      }
    }
  });

  async function handleBgFile(e: Event) {
    const input = e.currentTarget as HTMLInputElement;
    const file = input.files?.[0];
    if (!file) return;
    uploadingBg = true;
    try {
      await app.uploadBackground(file);
    } catch {
      // error shown by app.say
    } finally {
      uploadingBg = false;
      input.value = "";
    }
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

  // ------------------------------------------------------------ client patches
  let patches = $state<PatchRow[]>([]);

  async function loadPatches() {
    patches = await api.listPatches().catch(() => []);
  }

  function togglePatch(patch: PatchRow, on: boolean) {
    if (!s) return;
    const rest = s.enabled_patches.filter((id) => id !== patch.id);
    app.updateSettings({ enabled_patches: on ? [...rest, patch.id] : rest });
  }

  // ------------------------------------------------------------ server hubs
  const OFFICIAL_HUB = "https://hub.spacestation14.com/";
  let newHubUrl = $state("");
  let hubError = $state<string | null>(null);

  const hubs = $derived(s?.hubs ?? []);

  function isOfficial(url: string) {
    return url === OFFICIAL_HUB || url === OFFICIAL_HUB.slice(0, -1);
  }

  function toggleHub(index: number, enabled: boolean) {
    if (!s) return;
    const next = s.hubs.map((h, i) => (i === index ? { ...h, enabled } : h));
    app.updateSettings({ hubs: next });
    void app.refreshServers();
  }

  function removeHub(index: number) {
    if (!s) return;
    const next = s.hubs.filter((_, i) => i !== index);
    app.updateSettings({ hubs: next });
    void app.refreshServers();
  }

  function addPresetHub(url: string) {
    if (!s) return;
    hubError = null;
    const normalized = url.endsWith("/") ? url : `${url}/`;
    if (hubs.some((h) => h.url.toLowerCase() === normalized.toLowerCase())) {
      hubError = t("opt.hubsDuplicate");
      return;
    }
    const next = [...hubs, { url: normalized, enabled: true }];
    app.updateSettings({ hubs: next });
    void app.refreshServers();
  }

  function addHub(e: SubmitEvent) {
    e.preventDefault();
    hubError = null;
    const raw = newHubUrl.trim();
    if (!raw) return;

    if (!/^https?:\/\/[^\s/$.?#].[^\s]*$/i.test(raw)) {
      hubError = t("opt.hubsBadUrl");
      return;
    }
    const normalized = raw.endsWith("/") ? raw : `${raw}/`;
    if (hubs.some((h) => h.url.toLowerCase() === normalized.toLowerCase())) {
      hubError = t("opt.hubsDuplicate");
      return;
    }

    const next = [...hubs, { url: normalized, enabled: true }];
    app.updateSettings({ hubs: next });
    void app.refreshServers();
    newHubUrl = "";
  }

  // ------------------------------------------------------------ data
  let storage = $state<StorageRow | null>(null);
  let cleaning = $state(false);
  let cleaningAll = $state(false);
  let confirmCleanAll = $state(false);
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
  async function cleanAll() {
    if (!confirmCleanAll) {
      confirmCleanAll = true;
      setTimeout(() => (confirmCleanAll = false), 4000);
      return;
    }
    confirmCleanAll = false;
    cleaningAll = true;
    cleanResult = null;
    try {
      const r = await api.cleanStorageAll();
      cleanResult =
        r.freed > 0 ? t("opt.cleaned", { size: formatBytes(r.freed) }) : t("opt.cleanedNothing");
      await loadStorage();
    } catch (e) {
      cleanResult = errorText(e);
    } finally {
      cleaningAll = false;
    }
  }

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

  let showOptDetails = $state(false);

  function setOptimizationProfile(profile: string) {
    if (profile === "iskander") {
      app.updateSettings({
        opt_profile: "iskander",
        opt_server_gc: true,
        opt_fast_lighting: true,
        opt_unlock_fps: true,
        opt_high_priority: true,
      });
    } else if (profile === "potato") {
      app.updateSettings({
        opt_profile: "potato",
        opt_server_gc: true,
        opt_fast_lighting: true,
        opt_unlock_fps: true,
        opt_high_priority: true,
      });
    } else if (profile === "max_fps") {
      app.updateSettings({
        opt_profile: "max_fps",
        opt_server_gc: true,
        opt_fast_lighting: false,
        opt_unlock_fps: true,
        opt_high_priority: true,
      });
    } else if (profile === "off" || profile === "none") {
      app.updateSettings({
        opt_profile: "off",
        opt_server_gc: false,
        opt_fast_lighting: false,
        opt_unlock_fps: false,
        opt_high_priority: false,
      });
    } else if (profile === "custom") {
      app.updateSettings({ opt_profile: "custom" });
      showOptDetails = true;
    } else {
      app.updateSettings({
        opt_profile: "balanced",
        opt_server_gc: true,
        opt_fast_lighting: false,
        opt_unlock_fps: false,
        opt_high_priority: true,
      });
    }
  }

  function updateOptField(partial: Partial<import("./api").Settings>) {
    app.updateSettings({
      ...partial,
      opt_profile: "custom",
    });
  }

  function getProfileDescription(profile: string): string {
    switch (profile) {
      case "iskander":
        return t("opt.profile.iskanderDesc");
      case "potato":
        return t("opt.profile.potatoDesc");
      case "max_fps":
        return t("opt.profile.max_fpsDesc");
      case "off":
      case "none":
        return t("opt.profile.offDesc");
      case "custom":
        return t("opt.profile.customDesc");
      default:
        return t("opt.profile.balancedDesc");
    }
  }
</script>

{#if s}
  <div class="tab">
    {#if app.intelWarning}
      <div class="intel-card">
        <div class="intel-card-header">
          <strong>{t("opt.intelWarningTitle")}</strong>
        </div>
        <p class="intel-card-desc">{t("opt.intelWarningDesc")}</p>
        <button class="btn small" onclick={() => app.dismissIntelWarning()}>
          {t("opt.intelWarningDismiss")}
        </button>
      </div>
    {/if}

    <!-- ------------------------------------------------------------ general -->
    <section>
      <h2>{t("opt.general")}</h2>

      <label class="field">
        <span>{t("opt.language")}</span>
        <select value={s.language} onchange={(e) => app.updateSettings({ language: e.currentTarget.value })}>
          <option value="auto">{t("opt.langAuto")}</option>
          {#each UI_LANGUAGES as code (code)}
            <option value={code}>{nativeName(code)}</option>
          {/each}
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

      <label class="field">
        <span>{t("opt.launcherUiScale")}</span>
        <select
          value={s.launcher_ui_scale ?? 100}
          onchange={(e) => app.updateSettings({ launcher_ui_scale: Number(e.currentTarget.value) })}
        >
          <option value={75}>75%</option>
          <option value={80}>80%</option>
          <option value={90}>90%</option>
          <option value={100}>100%</option>
          <option value={110}>110%</option>
          <option value={125}>125%</option>
          <option value={150}>150%</option>
          <option value={175}>175%</option>
          <option value={200}>200%</option>
        </select>
      </label>

      <label class="field">
        <span>{t("opt.autoRefresh")}</span>
        <select
          value={s.auto_refresh_interval}
          onchange={(e) => app.updateSettings({ auto_refresh_interval: Number(e.currentTarget.value) })}
        >
          <option value={0}>{t("opt.autoRefreshOff")}</option>
          <option value={15}>{t("opt.autoRefresh15")}</option>
          <option value={30}>{t("opt.autoRefresh30")}</option>
          <option value={60}>{t("opt.autoRefresh60")}</option>
          <option value={120}>{t("opt.autoRefresh120")}</option>
        </select>
      </label>

      <Option
        label={t("opt.confirmConnect")}
        hint={t("opt.confirmConnectHint")}
        checked={s.confirm_connect}
        onchange={(v) => app.updateSettings({ confirm_connect: v })}
      />
      <Option label={t("opt.autostart")} hint={t("opt.autostartHint")} checked={autostart} onchange={setAutostart} />
      <Option label={t("opt.links")} hint={t("opt.linksHint")} checked={links} onchange={setLinks} />
    </section>

    <!-- ------------------------------------------------------------ client -->
    <section>
      <h2>{t("opt.client")}</h2>
      <p class="sub">{t("opt.clientDesc")}</p>

      <label class="field">
        <span>{t("opt.windowMode")}</span>
        <select
          value={s.client_window_mode}
          onchange={(e) => app.updateSettings({ client_window_mode: e.currentTarget.value })}
        >
          <option value="default">{t("opt.windowDefault")}</option>
          <option value="windowed">{t("opt.windowWindowed")}</option>
          <option value="fullscreen">{t("opt.windowFullscreen")}</option>
          <option value="borderless">{t("opt.windowBorderless")}</option>
        </select>
      </label>

      <label class="field">
        <span>{t("opt.uiScale")}</span>
        <select
          value={s.client_ui_scale}
          onchange={(e) => app.updateSettings({ client_ui_scale: e.currentTarget.value })}
        >
          <option value="auto">{t("opt.uiScaleAuto")}</option>
          <option value="0.75">75%</option>
          <option value="1.0">100%</option>
          <option value="1.25">125%</option>
          <option value="1.5">150%</option>
          <option value="1.75">175%</option>
          <option value="2.0">200%</option>
        </select>
      </label>

      <label class="field">
        <span>{t("opt.maxFps")}</span>
        <select
          value={s.client_max_fps}
          onchange={(e) => app.updateSettings({ client_max_fps: Number(e.currentTarget.value) })}
        >
          <option value={0}>{t("opt.maxFpsUncapped")}</option>
          <option value={60}>60 FPS</option>
          <option value={75}>75 FPS</option>
          <option value={120}>120 FPS</option>
          <option value={144}>144 FPS</option>
          <option value={165}>165 FPS</option>
          <option value={240}>240 FPS</option>
        </select>
      </label>

      <Option
        label={t("opt.muteUnfocused")}
        hint={t("opt.muteUnfocusedHint")}
        checked={s.client_mute_unfocused}
        onchange={(v) => app.updateSettings({ client_mute_unfocused: v })}
      />
      <Option
        label={t("opt.closeOnPlay")}
        hint={t("opt.closeOnPlayHint")}
        checked={s.close_on_play}
        onchange={(v) => app.updateSettings({ close_on_play: v })}
      />
      <Option
        label={t("opt.minimize")}
        hint={t("opt.minimizeHint")}
        checked={s.minimize_on_play}
        onchange={(v) => app.updateSettings({ minimize_on_play: v })}
      />
      <Option
        label={t("opt.compat")}
        hint={t("opt.compatHint")}
        checked={s.compat_mode}
        onchange={(v) => app.updateSettings({ compat_mode: v })}
      />
      <Option
        label={t("opt.discordRpc")}
        hint={t("opt.discordRpcHint")}
        checked={s.discord_rpc}
        onchange={(v) => app.updateSettings({ discord_rpc: v })}
      />
      <Option
        label={t("opt.ignoreRedial")}
        hint={t("opt.ignoreRedialHint")}
        checked={s.ignore_redial}
        onchange={(v) => app.updateSettings({ ignore_redial: v })}
      />
      <label class="field">
        <span>{t("opt.extraArgs")}</span>
        <input
          type="text"
          placeholder={t("opt.extraArgsPlaceholder")}
          value={s.extra_args}
          oninput={(e) => app.updateSettings({ extra_args: e.currentTarget.value })}
        />
        <small class="sub">{t("opt.extraArgsDesc")}</small>
      </label>
    </section>

    <!-- ------------------------------------------------------------ optimization -->
    <section>
      <h2>{t("opt.optimization")}</h2>
      <p class="sub">{t("opt.optProfileDesc")}</p>

      <label class="field">
        <span>{t("opt.optProfile")}</span>
        <select
          value={s.opt_profile}
          onchange={(e) => setOptimizationProfile(e.currentTarget.value)}
        >
          <option value="balanced">{t("opt.profile.balanced")}</option>
          <option value="max_fps">{t("opt.profile.max_fps")}</option>
          <option value="potato">{t("opt.profile.potato")}</option>
          <option value="iskander">{t("opt.profile.iskander")}</option>
          <option value="off">{t("opt.profile.off")}</option>
          <option value="custom">{t("opt.profile.custom")}</option>
        </select>
        <small class="sub">{getProfileDescription(s.opt_profile)}</small>
      </label>

      {#if s.opt_profile === "custom" || showOptDetails}
        <div class="opt-details">
          <Option
            label={t("opt.optServerGc")}
            hint={t("opt.optServerGcHint")}
            checked={s.opt_server_gc}
            onchange={(v) => updateOptField({ opt_server_gc: v })}
          />
          <Option
            label={t("opt.optFastLighting")}
            hint={t("opt.optFastLightingHint")}
            checked={s.opt_fast_lighting}
            onchange={(v) => updateOptField({ opt_fast_lighting: v })}
          />
          <Option
            label={t("opt.optUnlockFps")}
            hint={t("opt.optUnlockFpsHint")}
            checked={s.opt_unlock_fps}
            onchange={(v) => updateOptField({ opt_unlock_fps: v })}
          />
          <Option
            label={t("opt.optHighPriority")}
            hint={t("opt.optHighPriorityHint")}
            checked={s.opt_high_priority}
            onchange={(v) => updateOptField({ opt_high_priority: v })}
          />
        </div>
      {/if}

      {#if s.opt_profile !== "custom"}
        <div class="opt-toggle-row">
          <button class="btn secondary sm" onclick={() => (showOptDetails = !showOptDetails)}>
            {showOptDetails ? "▲ " + t("opt.optDetails") : "▼ " + t("opt.optDetails")}
          </button>
        </div>
      {/if}
    </section>

    <ErpPromo />

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

      <div class="field">
        <span>{t("opt.background")}</span>
        <div class="bg-mode-row">
          <button
            class="btn"
            class:primary={s.bg_type === "space"}
            onclick={() => app.updateSettings({ bg_type: "space" })}
          >
            {t("opt.bgSpace")}
          </button>
          <button
            class="btn"
            class:primary={s.bg_type === "custom"}
            onclick={() => app.updateSettings({ bg_type: "custom" })}
          >
            {t("opt.bgCustom")}
          </button>
          <button
            class="btn"
            class:primary={s.bg_type === "none"}
            onclick={() => app.updateSettings({ bg_type: "none" })}
          >
            {t("opt.bgNone")}
          </button>
        </div>
      </div>

      {#if s.bg_type === "custom" || s.bg_type === "space"}
        <div class="bg-controls">
          <label class="field range-field">
            <span>{t("opt.bgDim", { value: s.bg_dim })}</span>
            <input
              type="range"
              min="0"
              max="90"
              step="5"
              value={s.bg_dim}
              oninput={(e) => app.updateSettings({ bg_dim: Number(e.currentTarget.value) })}
            />
          </label>
          {#if s.bg_type === "custom"}
            <label class="field range-field">
              <span>{t("opt.bgBlur", { value: s.bg_blur })}</span>
              <input
                type="range"
                min="0"
                max="25"
                step="1"
                value={s.bg_blur}
                oninput={(e) => app.updateSettings({ bg_blur: Number(e.currentTarget.value) })}
              />
            </label>
          {/if}
        </div>
      {/if}

      {#if s.bg_type === "custom"}
        <div class="bg-gallery-section">
          <div class="bg-actions-row">
            <input
              type="file"
              accept="image/png,image/jpeg,image/webp,image/gif"
              bind:this={bgFileInput}
              onchange={handleBgFile}
              style="display: none;"
            />
            <button
              class="btn primary"
              disabled={uploadingBg}
              onclick={() => bgFileInput?.click()}
            >
              {uploadingBg ? "..." : "+ " + t("opt.bgUpload")}
            </button>
            <button class="btn" onclick={() => app.resetBackground()}>
              ↩ {t("opt.bgReset")}
            </button>
            <button class="btn" onclick={() => api.openBackgroundsFolder()}>
              📁 {t("opt.bgFolder")}
            </button>
          </div>

          {#if app.backgrounds.length === 0}
            <p class="sub small empty-bg-hint">{t("opt.bgEmpty")}</p>
          {:else}
            <div class="bg-grid">
              {#each app.backgrounds as bg (bg.id)}
                <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
                <div
                  class="bg-card"
                  class:active={s.bg_custom_name === bg.id}
                  onclick={() => app.selectBackground(bg.id)}
                >
                  <div
                    class="bg-thumb"
                    style:background-image={bgThumbnails[bg.id] ? `url('${bgThumbnails[bg.id]}')` : "none"}
                  >
                    {#if !bgThumbnails[bg.id]}
                      <span class="sub small">...</span>
                    {/if}
                    {#if s.bg_custom_name === bg.id}
                      <span class="active-badge">{t("opt.bgActive")}</span>
                    {/if}
                  </div>
                  <div class="bg-card-info">
                    <span class="bg-card-name" title={bg.name}>{bg.name}</span>
                    <span class="sub small">{formatBytes(bg.file_size)}</span>
                  </div>
                  <button
                    class="btn icon delete-btn"
                    title={t("opt.bgDelete")}
                    aria-label={t("opt.bgDelete")}
                    onclick={(e) => {
                      e.stopPropagation();
                      void app.deleteBackground(bg.id);
                    }}
                  >
                    ✕
                  </button>
                </div>
              {/each}
            </div>
          {/if}
        </div>
      {/if}
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

    <!-- ------------------------------------------------------------ server hubs -->
    <section>
      <h2>{t("opt.hubs")}</h2>
      <p class="sub">{t("opt.hubsDesc")}</p>

      <ul class="hubs">
        {#each hubs as hub, i (hub.url)}
          <li>
            <input
              type="checkbox"
              checked={hub.enabled}
              onchange={(e) => toggleHub(i, e.currentTarget.checked)}
              aria-label={hub.url}
            />
            <div class="grow">
              <span class="hub-url">{hub.url}</span>
              {#if isOfficial(hub.url)}
                <span class="sub small">({t("opt.hubsDefault")})</span>
              {/if}
            </div>
            {#if !isOfficial(hub.url)}
              <button
                class="btn icon"
                title={t("opt.hubsRemove")}
                aria-label={t("opt.hubsRemove")}
                onclick={() => removeHub(i)}
              >
                ✕
              </button>
            {/if}
          </li>
        {/each}
      </ul>

      <form class="hub-form" onsubmit={addHub}>
        <input
          type="text"
          placeholder={t("opt.hubsPlaceholder")}
          bind:value={newHubUrl}
          spellcheck="false"
        />
        <button class="btn" type="submit" disabled={!newHubUrl.trim()}>
          {t("opt.hubsAdd")}
        </button>
      </form>
      <div class="hub-presets">
        <span class="sub small">{t("opt.hubsPresets")}:</span>
        <button
          type="button"
          class="btn small"
          onclick={() => addPresetHub("https://hub.corvax.fun/")}
        >
          {t("opt.hubsCorvax")}
        </button>
        <button
          type="button"
          class="btn small"
          onclick={() => addPresetHub("https://hub.spacestories.club/")}
        >
          {t("opt.hubsSpaceStories")}
        </button>
      </div>

      {#if recommendedHubs.length > 0}
        <div class="recommended-hubs">
          <span class="sub small">{t("opt.recommendedHubs")}:</span>
          <div class="rec-list">
            {#each recommendedHubs as rec (rec.url)}
              {@const isAdded = hubs.some((h) => h.url.toLowerCase().replace(/\/$/, "") === rec.url.toLowerCase().replace(/\/$/, ""))}
              <div class="rec-row">
                <div class="rec-text">
                  <span class="rec-name">{rec.name}</span>
                  <span class="sub small">{rec.url}</span>
                </div>
                <button
                  type="button"
                  class="btn small soft"
                  disabled={isAdded}
                  onclick={() => addPresetHub(rec.url)}
                >
                  {isAdded ? "✓" : t("opt.addHub")}
                </button>
              </div>
            {/each}
          </div>
        </div>
      {/if}
      {#if hubError}
        <div class="error small">{hubError}</div>
      {/if}
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
                    {pack.id}{#if pack.problems.length === 0} · {t("pack.files", { count: pack.files })} · {pack.cultures.map(nativeName).join(", ")}{/if}
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

    <!-- ------------------------------------------------------------ client patches -->
    <section>
      <h2>{t("opt.patches")}</h2>
      <p class="sub">{t("opt.patchesDesc")}</p>

      {#if patches.length === 0}
        <p class="sub">{t("opt.patchesEmpty")}</p>
      {:else}
        <ul class="patches-list">
          {#each patches as patch (patch.id)}
            <li>
              <input
                type="checkbox"
                checked={s.enabled_patches.includes(patch.id)}
                onchange={(e) => togglePatch(patch, e.currentTarget.checked)}
                aria-label={patch.name}
              />
              <div class="grow">
                <div>
                  <b>{patch.name}</b>
                  <span class="sub small">· {formatBytes(patch.file_size)}</span>
                </div>
                <div class="sub small code">{patch.id}</div>
              </div>
            </li>
          {/each}
        </ul>
      {/if}
      <div class="row">
        <button class="btn primary" onclick={() => (app.tab = "mods")}>{t("tab.mods")}</button>
        <button class="btn open-r" onclick={() => api.openPatchesFolder()}>{t("opt.patchesOpen")}</button>
        <button class="btn open-l" onclick={loadPatches}>{t("opt.patchesRefresh")}</button>
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
        <button class="btn" onclick={clean} disabled={cleaning || cleaningAll || app.busy}>
          {cleaning ? t("opt.cleaning") : t("opt.clean")}
        </button>
        <button class="btn" onclick={cleanAll} disabled={cleaning || cleaningAll || app.busy}>
          {cleaningAll ? t("opt.cleaning") : confirmCleanAll ? t("opt.cleanAllConfirm") : t("opt.cleanAll")}
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
    padding: 16px 14px 28px;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 14px;
  }
  section {
    display: flex;
    flex-direction: column;
    gap: 12px;
    width: 100%;
    max-width: 720px;
    padding: 16px 18px 18px;
    background: var(--tint);
    border: 1px solid var(--line);
    border-radius: 12px;
  }
  section > h2 {
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--sub);
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
    padding: 12px 14px;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    background: var(--tint);
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
    padding: 8px 10px;
    background: var(--tint);
    border: 1px solid var(--line);
    border-radius: var(--radius);
  }
  .packs li.bad {
    border-color: color-mix(in srgb, var(--bad) 60%, transparent);
  }
  .patches-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .patches-list li {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 10px;
    background: var(--tint);
    border: 1px solid var(--line);
    border-radius: var(--radius);
  }
  .code {
    font-family: "Cascadia Mono", Consolas, monospace;
  }
  .hubs {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .hubs li {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 10px;
    background: var(--tint);
    border: 1px solid var(--line);
    border-radius: var(--radius);
  }
  .hub-form {
    display: flex;
    gap: 8px;
    align-items: center;
  }
  .hub-presets {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 6px;
    flex-wrap: wrap;
  }
  .hub-form input[type="text"] {
    flex: 1;
    min-width: 200px;
  }
  .hub-url {
    word-break: break-all;
    font-family: "Cascadia Mono", Consolas, monospace;
    font-size: 13px;
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
    border-color: color-mix(in srgb, var(--accent) 40%, transparent);
  }
  pre {
    margin: 4px 0 0;
    padding: 10px;
    max-width: 100%;
    background: var(--tint);
    border: 1px solid var(--line);
    border-radius: var(--radius-sm);
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
  .opt-details {
    display: flex;
    flex-direction: column;
    gap: 10px;
    margin-top: 10px;
    padding: 12px;
    background: var(--tint);
    border: 1px solid var(--line);
    border-radius: var(--radius-sm);
  }
  .opt-toggle-row {
    margin-top: 6px;
  }
  .bg-mode-row {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
    margin-top: 4px;
  }
  .bg-controls {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 12px 14px;
    background: var(--tint);
    border: 1px solid var(--line);
    border-radius: var(--radius-sm);
  }
  .range-field {
    width: 100%;
  }
  .range-field input[type="range"] {
    width: 100%;
    max-width: 320px;
    cursor: pointer;
    accent-color: var(--accent);
  }
  .bg-gallery-section {
    display: flex;
    flex-direction: column;
    gap: 12px;
    margin-top: 4px;
  }
  .bg-actions-row {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
    align-items: center;
  }
  .empty-bg-hint {
    padding: 8px 0;
  }
  .bg-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(180px, 1fr));
    gap: 12px;
    margin-top: 6px;
  }
  .bg-card {
    position: relative;
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 8px;
    background: var(--tint);
    border: 1px solid var(--line);
    border-radius: var(--radius-sm);
    cursor: pointer;
    transition: border-color 0.15s, background 0.15s;
  }
  .bg-card:hover {
    background: var(--tint-hover);
    border-color: var(--border-high);
  }
  .bg-card.active {
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 12%, var(--tint));
  }
  .bg-thumb {
    position: relative;
    width: 100%;
    height: 100px;
    background-size: cover;
    background-position: center;
    border-radius: 4px;
    background-color: var(--ctrl-mid);
    display: flex;
    align-items: center;
    justify-content: center;
    overflow: hidden;
  }
  .active-badge {
    position: absolute;
    top: 6px;
    left: 6px;
    background: var(--accent);
    color: var(--on-accent);
    font-size: 11px;
    font-weight: 600;
    padding: 2px 6px;
    border-radius: 4px;
    letter-spacing: 0.04em;
    text-transform: uppercase;
  }
  .bg-card-info {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .bg-card-name {
    font-size: 13px;
    font-weight: 500;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .delete-btn {
    position: absolute;
    top: 12px;
    right: 12px;
    min-height: 24px;
    height: 24px;
    width: 24px;
    padding: 0;
    font-size: 11px;
    background: rgba(0, 0, 0, 0.65);
    color: #fff;
    border-radius: 4px;
    border: none;
    opacity: 0.8;
  }
  .delete-btn:hover {
    opacity: 1;
    background: var(--bad);
  }
  .intel-card {
    margin-bottom: 16px;
    padding: 12px 16px;
    border-radius: var(--radius);
    border: 1px solid color-mix(in srgb, var(--warn) 40%, transparent);
    background: color-mix(in srgb, var(--warn) 10%, transparent);
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .intel-card-header {
    color: var(--warn);
  }
  .intel-card-desc {
    margin: 0;
    font-size: 13px;
    line-height: 1.4;
    user-select: text;
  }
  .recommended-hubs {
    margin-top: 14px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .rec-list {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .rec-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    padding: 8px 12px;
    background: var(--tint);
    border-radius: var(--radius-sm);
  }
  .rec-text {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .rec-name {
    font-weight: 500;
    font-size: 13px;
  }
</style>
