<script lang="ts">
  import { onMount } from "svelte";
  import { api, errorText, type StorageRow } from "./api";
  import { formatBytes } from "./format";
  import { t } from "./i18n.svelte";
  import Modal from "./Modal.svelte";
  import { app } from "./store.svelte";
  import { applyTheme, themes } from "./themes";

  let { onclose }: { onclose: () => void } = $props();

  let guestName = $state(app.settings?.guest_name ?? "");
  let compat = $state(app.settings?.compat_mode ?? false);
  let minimize = $state(app.settings?.minimize_on_play ?? true);
  let theme = $state(app.settings?.theme ?? "dark");
  let language = $state(app.settings?.language ?? "auto");
  let autostart = $state(false);
  let error = $state<string | null>(null);

  let storage = $state<StorageRow | null>(null);
  let cleaning = $state(false);
  let cleanResult = $state<string | null>(null);

  onMount(() => {
    void api.getAutostart().then((v) => (autostart = v)).catch(() => {});
    void refreshStorage();
  });

  // Preview while choosing; closing without saving puts the saved theme back.
  $effect(() => {
    applyTheme(theme);
    return () => applyTheme(app.settings?.theme ?? "dark");
  });

  async function refreshStorage() {
    storage = await api.storageInfo().catch(() => null);
  }

  async function clean() {
    cleaning = true;
    cleanResult = null;
    error = null;
    try {
      const r = await api.cleanStorage();
      cleanResult =
        r.freed > 0 ? t("set.cleaned", { size: formatBytes(r.freed) }) : t("set.cleanedNothing");
      await refreshStorage();
    } catch (e) {
      error = errorText(e);
    } finally {
      cleaning = false;
    }
  }

  async function save() {
    if (!app.settings) return;
    try {
      // Autostart lives in Windows, not in our settings file.
      if (autostart !== (await api.getAutostart())) {
        autostart = await api.setAutostart(autostart);
      }
      await app.saveSettings({
        ...app.settings,
        guest_name: guestName,
        compat_mode: compat,
        minimize_on_play: minimize,
        theme,
        language,
      });
      onclose();
    } catch (e) {
      error = errorText(e);
    }
  }
</script>

<Modal title={t("set.title")} {onclose}>
  <div class="form">
    <label>
      <span>{t("set.guestName")}</span>
      <input type="text" bind:value={guestName} maxlength="32" />
      <small class="dim">{t("set.guestHint")}</small>
    </label>

    <div class="pair">
      <label>
        <span>{t("set.theme")}</span>
        <select bind:value={theme}>
          {#each themes as th}<option value={th.id}>{t(th.label)}</option>{/each}
        </select>
      </label>
      <label>
        <span>{t("set.language")}</span>
        <select bind:value={language}>
          <option value="auto">{t("set.langAuto")}</option>
          <option value="ru">Русский</option>
          <option value="en">English</option>
        </select>
      </label>
    </div>

    <label class="check">
      <input type="checkbox" bind:checked={compat} />
      <span>
        {t("set.compat")}
        <small class="dim">{t("set.compatHint")}</small>
      </span>
    </label>

    <label class="check">
      <input type="checkbox" bind:checked={minimize} />
      <span>
        {t("set.minimize")}
        <small class="dim">{t("set.minimizeHint")}</small>
      </span>
    </label>

    <label class="check">
      <input type="checkbox" bind:checked={autostart} />
      <span>
        {t("set.autostart")}
        <small class="dim">{t("set.autostartHint")}</small>
      </span>
    </label>

    <div class="storage">
      <b>{t("set.storage")}</b>
      {#if storage}
        <div>{t("set.storageTotal", { size: formatBytes(storage.total) })}</div>
        <small class="dim">
          {t("set.storageDetails", {
            content: formatBytes(storage.content),
            engines: formatBytes(storage.engines),
            modules: formatBytes(storage.modules),
            other: formatBytes(storage.other),
          })}
        </small>
      {/if}
      <div class="row">
        <button onclick={clean} disabled={cleaning || app.busy}>
          {cleaning ? t("set.cleaning") : t("set.clean")}
        </button>
        {#if cleanResult}<span class="dim">{cleanResult}</span>{/if}
      </div>
    </div>

    <div class="about dim">
      <div>BebraLauncher {app.info?.version ?? ""}</div>
      <div class="path">{t("set.data", { path: app.info?.data_dir ?? "" })}</div>
      <button class="link" onclick={() => api.openLogsFolder()}>{t("set.openLogs")}</button>
    </div>

    {#if error}<div class="error">{error}</div>{/if}
    <div class="actions">
      <button class="primary" onclick={save}>{t("set.save")}</button>
      <button onclick={onclose}>{t("set.cancel")}</button>
    </div>
  </div>
</Modal>

<style>
  .form {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  label {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .pair {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 12px;
  }
  label.check {
    flex-direction: row;
    align-items: flex-start;
    gap: 8px;
  }
  label.check span {
    display: flex;
    flex-direction: column;
  }
  small {
    font-size: 12px;
  }
  .storage {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 10px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
  }
  .storage .row {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-top: 4px;
  }
  .about {
    font-size: 12px;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .path {
    user-select: text;
    word-break: break-all;
  }
  .link {
    background: none;
    border: none;
    padding: 0;
    color: var(--accent);
    align-self: flex-start;
  }
  .actions {
    display: flex;
    gap: 8px;
  }
</style>
