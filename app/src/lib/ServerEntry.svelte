<script lang="ts">
  import { api, type ServerRow } from "./api";
  import {
    formatPlayed,
    gridTemplate,
    pingClass,
    playersText,
    roundText,
    serverTitle,
  } from "./format";
  import Icon from "./Icon.svelte";
  import { t } from "./i18n.svelte";
  import { app } from "./store.svelte";

  let {
    server,
    raise = false,
  }: {
    server: ServerRow;
    /** Show the "raise to top" button (the favorites tab). */
    raise?: boolean;
  } = $props();

  let expanded = $state(false);
  let root = $state<HTMLElement>();
  let copied = $state(false);

  const columns = $derived(app.columns);
  const template = $derived(gridTemplate(columns));
  const isFavorite = $derived(app.favorites.has(server.address));
  const details = $derived(app.details[server.address]);
  const ready = $derived(typeof details === "object" ? details : null);
  const ping = $derived(app.pings[server.address]);
  const round = $derived(roundText(server, app.now));
  const played = $derived(app.historyOf(server.address));
  // A favourite the hub does not list and that did not answer: nothing known about it.
  const silent = $derived(
    server.name === null && server.players === 0 && app.pings[server.address] === null,
  );

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

  async function copyAddress() {
    try {
      await navigator.clipboard.writeText(server.address);
      copied = true;
      setTimeout(() => (copied = false), 1500);
    } catch {
      // No clipboard access: the address is selectable text anyway.
    }
  }
</script>

<div class="entry" bind:this={root}>
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="head" style:grid-template-columns={template} onclick={() => (expanded = !expanded)}>
    <div class="cell name">
      <span class="chev" class:open={expanded}><Icon name="chevron" size={18} /></span>
      {#if isFavorite}<span class="fav" title={t("server.favoriteMark")}><Icon name="star" size={14} /></span>{/if}
      <span class="title">{serverTitle(server)}</span>
    </div>
    {#if columns.mode}
      <div class="cell center sub">{server.preset ?? ""}</div>
    {/if}
    {#if columns.map}
      <div class="cell center sub">{server.map ?? ""}</div>
    {/if}
    {#if columns.ping}
      <div class="cell center ping {pingClass(ping)}">
        {ping == null ? t("server.noPing") : t("server.ping", { ms: ping })}
      </div>
    {/if}
    {#if columns.round_time}
      <div class="cell center">{silent ? t("server.offline") : (round ?? "")}</div>
    {/if}
    {#if columns.players}
      <div class="cell right numeric">{silent ? "" : playersText(server)}</div>
    {/if}
    <div class="cell last">
      <!-- svelte-ignore a11y_consider_explicit_label -->
      <button
        class="btn"
        disabled={app.busy}
        onclick={(e) => {
          e.stopPropagation();
          void app.connect(server);
        }}
      >
        {t("server.connect")}
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
      </div>

      {#if server.tags.length > 0}
        <div class="tags">
          {#each server.tags as tag}<span class="tag">{tag}</span>{/each}
        </div>
      {/if}

      <div class="actions">
        <div class="links">
          {#each ready?.links ?? [] as link (link.url)}
            <button class="btn" onclick={() => api.openUrl(link.url)} title={link.url}>
              <span class="with-icon"><Icon name="external" size={16} />{link.name}</span>
            </button>
          {/each}
        </div>
        <div class="fav-actions">
          <button
            class="btn"
            class:open-r={raise}
            onclick={() => app.toggleFavorite(server.address)}
          >
            <span class="with-icon">
              <Icon name={isFavorite ? "star" : "star-outline"} size={18} />
              {isFavorite ? t("server.removeFavorite") : t("server.addFavorite")}
            </span>
          </button>
          {#if raise}
            <button class="btn open-l" onclick={() => app.raiseFavorite(server.address)}>
              {t("server.raise")}
            </button>
          {/if}
        </div>
      </div>
    </div>
  {/if}
</div>

<style>
  .entry {
    flex: none;
  }
  .head {
    display: grid;
    align-items: stretch;
    min-height: 42px;
    cursor: pointer;
  }
  .head:hover {
    background: color-mix(in srgb, var(--ctrl-mid) 35%, transparent);
  }
  .cell {
    display: flex;
    align-items: center;
    padding: 0 10px;
    min-width: 0;
    border-right: 2px solid var(--sep);
  }
  .cell.last {
    border-right: 0;
    justify-content: center;
  }
  .cell.center {
    justify-content: center;
    text-align: center;
  }
  .cell.right {
    justify-content: flex-end;
  }
  .numeric {
    font-family: "Cascadia Mono", Consolas, monospace;
    font-variant-numeric: tabular-nums;
  }
  .name {
    gap: 6px;
  }
  .title {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .chev {
    flex: none;
    display: grid;
    transform: rotate(-90deg);
    transition: transform 0.12s;
    color: var(--sub);
  }
  .chev.open {
    transform: none;
  }
  .fav {
    flex: none;
    color: var(--accent);
    display: grid;
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
    padding: 8px 12px 12px 40px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    border-top: 1px solid var(--scroll-sep);
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
    gap: 4px;
  }
  .tag {
    font-size: 12px;
    padding: 0 8px;
    background: var(--popup);
    border: 1px solid var(--ctrl-mid);
    color: var(--sub);
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
    margin-left: auto;
  }
  .with-icon {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
</style>
