<script lang="ts">
  import { api, type Columns, type ServerRow } from "./api";
  import {
    formatPlayed,
    gridTemplate,
    pingClass,
    playersText,
    serverTitle,
  } from "./format";
  import Icon from "./Icon.svelte";
  import { tagLabel, rpKey } from "./filters";
  import { i18n, t } from "./i18n.svelte";
  import RoundTimer from "./RoundTimer.svelte";
  import { app } from "./store.svelte";

  let {
    server,
    raise = false,
    columns: chosenColumns = undefined,
    template: chosenTemplate = undefined,
  }: {
    server: ServerRow;
    /** Show the "raise to top" button (the favorites tab). */
    raise?: boolean;
    /** The columns the table decided to show; defaults to the player's choice. */
    columns?: Columns;
    /** The CSS grid template of the row, shared with the table header. */
    template?: string;
  } = $props();

  let expanded = $state(false);
  let root = $state<HTMLElement>();
  let copied = $state(false);

  const columns = $derived(chosenColumns ?? app.columns);
  const template = $derived(chosenTemplate ?? gridTemplate(columns));
  const isFavorite = $derived(app.favorites.has(server.address));
  const details = $derived(app.details[server.address]);
  const ready = $derived(typeof details === "object" ? details : null);
  const ping = $derived(app.pings[server.address]);
  const played = $derived(app.historyOf(server.address));
  // A favourite the hub does not list and that did not answer: nothing known about it.
  const silent = $derived(
    server.name === null && server.players === 0 && app.pings[server.address] === null,
  );

  /** The row is a button: Enter and Space open it, like a click. */
  function onHeadKeydown(e: KeyboardEvent) {
    if (e.target !== e.currentTarget) return;
    if (e.key === "Enter" || e.key === " " || e.key === "Spacebar") {
      e.preventDefault();
      expanded = !expanded;
    }
  }

  // Opened by a link or a redial: unfold and show it.
  $effect(() => {
    if (app.focusAddress === server.address) {
      expanded = true;
      app.focusAddress = null;
      queueMicrotask(() => root?.scrollIntoView({ block: "nearest" }));
    }
  });

  $effect(() => {
    if (expanded) void app.loadDetails(server.address);
  });

  /** A tag as a person reads it: language by its own name, region and roleplay level translated. */
  function tagText(tag: string): string {
    const rp = /^rp:(.+)$/i.exec(tag);
    const key = rp ? rpKey(rp[1]) : null;
    if (key) return `${t("filters.rp")}: ${t(key)}`;
    return tagLabel(tag, i18n.lang);
  }

  async function copyAddress() {
    try {
      await navigator.clipboard.writeText(server.address);
      copied = true;
      setTimeout(() => (copied = false), 1500);
    } catch {
      // No clipboard access: the address is selectable text anyway.
    }
  }

  const customName = $derived(app.favoriteNameOf(server.address));
  const displayTitle = $derived(customName || serverTitle(server));

  const note = $derived(app.noteOf(server.address));
  let editingNote = $state(false);
  let noteDraft = $state("");

  function startEditNote() {
    noteDraft = note;
    editingNote = true;
  }

  function cancelEditNote() {
    editingNote = false;
  }

  function saveNote() {
    void app.setServerNote(server.address, noteDraft);
    editingNote = false;
  }

  function deleteNote() {
    void app.setServerNote(server.address, "");
    editingNote = false;
  }
</script>

<div class="entry" bind:this={root}>
  <div
    class="head"
    style:grid-template-columns={template}
    role="button"
    tabindex="0"
    aria-expanded={expanded}
    onclick={() => (expanded = !expanded)}
    onkeydown={onHeadKeydown}
  >
    <div class="cell name">
      <span class="chev" class:open={expanded}><Icon name="chevron" size={18} /></span>
      {#if isFavorite}<span class="fav" title={t("server.favoriteMark")}><Icon name="star" size={14} /></span>{/if}
      {#if note}<span class="note-badge" title={note}><Icon name="note" size={13} /></span>{/if}
      <span class="title">{displayTitle}</span>
    </div>
    {#if columns.mode}
      <div class="cell center sub">{server.preset ?? ""}</div>
    {/if}
    {#if columns.map}
      <div class="cell center sub">{server.map ?? ""}</div>
    {/if}
    {#if columns.ping}
      <div class="cell center ping {pingClass(ping)}" title={ping == null ? t("server.noPing") : t("server.pingTooltip", { ms: ping })}>
        {#if ping != null}<i class="dot"></i>{/if}
        {ping == null ? t("server.noPing") : t("server.ping", { ms: ping })}
      </div>
    {/if}
    {#if columns.round_time}
      <div class="cell center">
        {#if silent}{t("server.offline")}{:else}<RoundTimer {server} />{/if}
      </div>
    {/if}
    {#if columns.players}
      <div class="cell center numeric">{silent ? "" : playersText(server)}</div>
    {/if}
    <div class="cell last">
      <!-- svelte-ignore a11y_consider_explicit_label -->
      <button
        class="btn soft"
        class:accent={app.isRunning(server.address)}
        disabled={app.isGameBusy}
        onclick={(e) => {
          e.stopPropagation();
          void app.connect(server);
        }}
      >
        {app.isRunning(server.address) ? t("server.running") : t("server.connect")}
      </button>
    </div>
  </div>

  {#if expanded}
    <div class="body">
      {#if ready?.description}
        <p class="desc">{ready.description}</p>
      {:else if details === "failed"}
        <p class="desc sub">{t("server.descFailed")}</p>
      {:else if ready}
        <p class="desc sub">{t("server.descNone")}</p>
      {:else}
        <p class="desc sub">{t("server.descLoading")}</p>
      {/if}

      <div class="facts sub">
        <span class="addr">{server.address}</span>
        <button class="link" onclick={copyAddress}>
          {copied ? t("server.copied") : t("server.copyAddress")}
        </button>
        {#if played && played.play_seconds > 0}
          <span>{t("server.played", { time: formatPlayed(played.play_seconds) })}</span>
        {/if}
        {#if server.panic_bunker}<span class="warn">{t("server.bunker")}</span>{/if}
        {#if ready?.auth === "required"}
          <span class="warn">{t("server.authRequired")}</span>
        {:else if ready?.auth === "disabled"}
          <span>{t("server.authDisabled")}</span>
        {/if}
        {#if customName && customName !== serverTitle(server)}
          <span class="sub original-name">({serverTitle(server)})</span>
        {/if}
      </div>

      {#if server.tags.length > 0}
        <div class="tags">
          {#each server.tags as tag}
            <button
              type="button"
              class="tag btn-tag"
              title={t("filters.clickToFilter")}
              onclick={(e) => {
                e.stopPropagation();
                app.toggleTagFilter(tag);
              }}
            >
              {tagText(tag)}
            </button>
          {/each}
        </div>
      {/if}

      <div class="note-box">
        {#if editingNote}
          <div class="note-edit">
            <textarea
              class="note-textarea"
              bind:value={noteDraft}
              placeholder={t("server.notePlaceholder")}
              maxlength={500}
              rows={2}
            ></textarea>
            <div class="note-btns">
              <button class="btn small primary" onclick={saveNote}>{t("server.saveNote")}</button>
              {#if note}
                <button class="btn small ghost warn-text" onclick={deleteNote}>{t("server.deleteNote")}</button>
              {/if}
              <button class="btn small ghost" onclick={cancelEditNote}>{t("server.cancelNote")}</button>
            </div>
          </div>
        {:else if note}
          <div class="note-display">
            <span class="note-icon" title={t("server.note")}><Icon name="note" size={15} /></span>
            <span class="note-text">{note}</span>
            <button class="link note-edit-link" onclick={startEditNote}>{t("server.editNote")}</button>
          </div>
        {:else}
          <div class="note-empty">
            <button class="link add-note-link" onclick={startEditNote}>
              <span class="with-icon"><Icon name="note" size={14} />{t("server.addNote")}</span>
            </button>
          </div>
        {/if}
      </div>

      <div class="actions">
        <div class="links">
          {#each ready?.links ?? [] as link (link.url)}
            <button class="btn ghost" onclick={() => api.openUrl(link.url)} title={link.url}>
              <span class="with-icon"><Icon name="external" size={16} />{link.name}</span>
            </button>
          {/each}
        </div>
        <div class="fav-actions">
          {#if raise}
            <button
              class="btn icon"
              title={t("server.moveUp")}
              aria-label={t("server.moveUp")}
              onclick={() => app.moveFavorite(server.address, -1)}
            >
              <Icon name="up" size={16} />
            </button>
            <button
              class="btn icon"
              title={t("server.moveDown")}
              aria-label={t("server.moveDown")}
              onclick={() => app.moveFavorite(server.address, 1)}
            >
              <Icon name="down" size={16} />
            </button>
            <button
              class="btn"
              onclick={() => (app.dialog = { kind: "editfav", address: server.address, name: customName ?? server.name })}
            >
              {t("server.editFav")}
            </button>
            <button class="btn" onclick={() => app.raiseFavorite(server.address)}>
              {t("server.raise")}
            </button>
          {:else if isFavorite}
            <button
              class="btn"
              onclick={() => (app.dialog = { kind: "editfav", address: server.address, name: customName ?? server.name })}
            >
              {t("server.editFav")}
            </button>
          {/if}
          <button
            class="btn"
            onclick={() => app.toggleFavorite(server.address)}
          >
            <span class="with-icon">
              <Icon name={isFavorite ? "star" : "star-outline"} size={18} />
              {isFavorite ? t("server.removeFavorite") : t("server.addFavorite")}
            </span>
          </button>
        </div>
      </div>
    </div>
  {/if}
</div>

<style>
  .entry {
    flex: none;
    margin: 0 10px 0 6px;
    border-radius: var(--radius);
    border-bottom: 1px solid var(--line);
    transition: background 0.12s;
  }
  .entry:has(.body) {
    background: var(--tint);
    border-bottom-color: transparent;
  }
  .head {
    display: grid;
    align-items: stretch;
    min-height: 46px;
    padding: 0 0 0 8px;
    cursor: pointer;
    border-radius: var(--radius);
  }
  .head:hover {
    background: var(--tint);
  }
  .cell {
    display: flex;
    align-items: center;
    padding: 0 10px;
    min-width: 0;
  }
  .cell.last {
    justify-content: center;
  }
  .cell.center {
    justify-content: center;
    text-align: center;
    gap: 6px;
  }
  .cell.right {
    justify-content: flex-end;
  }
  .numeric {
    font-variant-numeric: tabular-nums;
  }
  .cell.sub {
    font-size: 13px;
  }
  .name {
    gap: 6px;
  }
  .title {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: 500;
  }
  .chev {
    flex: none;
    display: grid;
    transform: rotate(-90deg);
    transition: transform 0.14s;
    color: var(--muted);
  }
  .chev.open {
    transform: none;
  }
  .fav {
    flex: none;
    color: var(--accent);
    display: grid;
  }
  .note-badge {
    flex: none;
    color: var(--accent);
    opacity: 0.85;
    display: grid;
  }
  .ping {
    white-space: nowrap;
    font-size: 13px;
    font-variant-numeric: tabular-nums;
  }
  .dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: currentColor;
    box-shadow: 0 0 5px currentColor;
  }
  .ping.good {
    color: var(--good);
  }
  .ping.mid {
    color: var(--warn);
  }
  .ping.bad {
    color: var(--bad);
  }
  .ping.unknown {
    color: var(--sub);
  }
  .body {
    padding: 4px 16px 14px 44px;
    display: flex;
    flex-direction: column;
    gap: 10px;
    animation: fade-in 0.14s ease-out;
  }
  .desc {
    margin: 0;
    white-space: pre-wrap;
    word-break: break-word;
    user-select: text;
  }
  .facts {
    display: flex;
    flex-wrap: wrap;
    gap: 4px 14px;
    font-size: 13px;
  }
  .addr {
    user-select: text;
    word-break: break-all;
  }
  .tags {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .tag {
    font-size: 12px;
    font-weight: 500;
    padding: 1px 9px;
    background: var(--tint-hover);
    border-radius: 999px;
    color: var(--sub);
  }
  .btn-tag {
    cursor: pointer;
    border: 1px solid transparent;
    transition: all 0.15s ease;
  }
  .btn-tag:hover {
    border-color: var(--accent);
    color: var(--accent);
    background: var(--tint-hover);
  }
  .actions {
    display: flex;
    justify-content: space-between;
    align-items: flex-end;
    gap: 12px;
    flex-wrap: wrap;
  }
  .links {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  .fav-actions {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-left: auto;
  }
  .original-name {
    font-style: italic;
  }
  .with-icon {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .note-box {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .note-display {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    padding: 7px 11px;
    background: var(--tint);
    border: 1px solid color-mix(in srgb, var(--accent) 25%, var(--line));
    border-radius: var(--radius-sm);
    font-size: 13px;
  }
  .note-icon {
    flex: none;
    color: var(--accent);
    margin-top: 1px;
    display: grid;
  }
  .note-text {
    flex: 1;
    white-space: pre-wrap;
    word-break: break-word;
    user-select: text;
    line-height: 1.4;
  }
  .note-edit-link {
    flex: none;
    font-size: 12px;
    font-weight: 500;
    align-self: flex-start;
  }
  .note-edit {
    display: flex;
    flex-direction: column;
    gap: 8px;
    max-width: 600px;
  }
  .note-textarea {
    resize: vertical;
    width: 100%;
    min-height: 52px;
    font-size: 13px;
    box-sizing: border-box;
  }
  .note-btns {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .note-empty {
    display: flex;
  }
  .add-note-link {
    font-size: 12px;
    font-weight: 500;
    color: var(--sub);
  }
  .add-note-link:hover {
    color: var(--accent);
  }
  .warn-text {
    color: var(--warn);
  }
</style>
