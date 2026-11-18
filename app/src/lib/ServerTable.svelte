<script lang="ts">
  import type { Snippet } from "svelte";
  import type { ServerRow } from "./api";
  import { nextSort, type SortKey } from "./filters";
  import { gridTemplate } from "./format";
  import { t } from "./i18n.svelte";
  import { app } from "./store.svelte";
  import { compactColumns } from "./tableLayout";
  import ServerEntry from "./ServerEntry.svelte";

  let {
    rows,
    sortable = true,
    raise = false,
    onreorder = undefined,
    children,
  }: {
    /** Rows can be dragged by their handle; called with the address and the new index. */
    onreorder?: (address: string, toIndex: number) => void;
    rows: ServerRow[];
    /** Clicking a column header sorts by it. */
    sortable?: boolean;
    raise?: boolean;
    /** Shown under the rows: an empty-list message or buttons. */
    children?: Snippet;
  } = $props();

  let host = $state<HTMLElement>();
  let width = $state(1000);

  // 200% zoom or a small window: show fewer columns instead of squeezing the name and Connect.
  const columns = $derived(compactColumns(app.columns, width));
  const template = $derived(gridTemplate(columns));

  $effect(() => {
    const el = host;
    if (!el) return;
    if (el.clientWidth > 0) width = el.clientWidth;
    const observer = new ResizeObserver((entries) => {
      const next = entries[0]?.contentRect.width ?? 0;
      if (next > 0) width = next;
    });
    observer.observe(el);
    return () => observer.disconnect();
  });

  function sortBy(key: SortKey) {
    if (sortable) app.sort = nextSort(app.sort, key);
  }

  // Dragging by the handle. The lifted row follows the pointer; the rows it passes slide
  // out of its way. On release the row settles into its slot, and only then the order is
  // committed, in the same frame the transforms are cleared, so nothing jumps.
  type Drag = { from: number; to: number; dy: number; settling: boolean };
  let scrollEl = $state<HTMLElement>();
  let drag = $state<Drag | null>(null);
  let geo: { tops: number[]; hs: number[]; startY: number; startScroll: number } | null = null;
  let dragAddress = "";
  let lastY = 0;
  let raf = 0;

  const SETTLE_MS = 200;
  const reduced = () => matchMedia("(prefers-reduced-motion: reduce)").matches;

  function begin(e: PointerEvent, index: number) {
    const sc = scrollEl;
    if (!onreorder || !sc || e.button !== 0 || drag || rows.length < 2) return;
    e.preventDefault();
    e.stopPropagation();
    const base = sc.getBoundingClientRect();
    const slots = [...sc.querySelectorAll<HTMLElement>(":scope > .slot")];
    geo = {
      tops: slots.map((s) => s.getBoundingClientRect().top - base.top + sc.scrollTop),
      hs: slots.map((s) => s.offsetHeight),
      startY: e.clientY,
      startScroll: sc.scrollTop,
    };
    dragAddress = rows[index].address;
    lastY = e.clientY;
    drag = { from: index, to: index, dy: 0, settling: false };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
    window.addEventListener("pointercancel", onCancel);
    document.body.classList.add("reordering");
    raf = requestAnimationFrame(tick);
  }

  function recompute() {
    const sc = scrollEl;
    if (!drag || !geo || !sc || drag.settling) return;
    const { tops, hs } = geo;
    const i = drag.from;
    const n = tops.length;
    const raw = lastY - geo.startY + sc.scrollTop - geo.startScroll;
    const dy = Math.min(Math.max(raw, tops[0] - tops[i]), tops[n - 1] + hs[n - 1] - hs[i] - tops[i]);
    const center = tops[i] + dy + hs[i] / 2;
    let to = 0;
    for (let k = 0; k < n; k++) if (k !== i && tops[k] + hs[k] / 2 < center) to++;
    drag = { ...drag, dy, to };
  }

  /** Where the dragged row ends up relative to where it started. */
  function settleOffset(from: number, to: number): number {
    if (!geo) return 0;
    let sum = 0;
    if (to > from) for (let k = from + 1; k <= to; k++) sum += geo.hs[k];
    else for (let k = to; k < from; k++) sum -= geo.hs[k];
    return sum;
  }

  function shiftOf(k: number): number {
    if (!drag || !geo) return 0;
    const { from, to } = drag;
    if (k === from) return drag.dy;
    if (from < to && k > from && k <= to) return -geo.hs[from];
    if (from > to && k < from && k >= to) return geo.hs[from];
    return 0;
  }

  function tick() {
    const sc = scrollEl;
    if (!drag || drag.settling || !sc) return;
    const r = sc.getBoundingClientRect();
    const edge = 48;
    let v = 0;
    if (lastY < r.top + edge) v = -(1 - Math.max(lastY - r.top, 0) / edge);
    else if (lastY > r.bottom - edge) v = Math.min((lastY - (r.bottom - edge)) / edge, 1);
    if (v !== 0) {
      sc.scrollTop += v * 14;
      recompute();
    }
    raf = requestAnimationFrame(tick);
  }

  function onMove(e: PointerEvent) {
    lastY = e.clientY;
    recompute();
  }
  const onUp = () => finish(false);
  const onCancel = () => finish(true);

  function finish(cancel: boolean) {
    window.removeEventListener("pointermove", onMove);
    window.removeEventListener("pointerup", onUp);
    window.removeEventListener("pointercancel", onCancel);
    cancelAnimationFrame(raf);
    if (!drag) return;
    const { from } = drag;
    const to = cancel ? from : drag.to;
    drag = { from, to, dy: to === from ? 0 : settleOffset(from, to), settling: true };
    const address = dragAddress;
    setTimeout(() => {
      drag = null;
      geo = null;
      document.body.classList.remove("reordering");
      if (to !== from) onreorder?.(address, to);
    }, reduced() ? 0 : SETTLE_MS);
  }

  $effect(() => () => {
    window.removeEventListener("pointermove", onMove);
    window.removeEventListener("pointerup", onUp);
    window.removeEventListener("pointercancel", onCancel);
    cancelAnimationFrame(raf);
    document.body.classList.remove("reordering");
  });
</script>

{#snippet head(key: SortKey, label: string, center: boolean, title?: string)}
  <div
    class="cell hcell"
    class:center
    role="columnheader"
    aria-sort={sortable && app.sort.key === key ? (app.sort.desc ? "descending" : "ascending") : "none"}
  >
    <button class="hbtn" class:center {title} disabled={!sortable} onclick={() => sortBy(key)}>
      {label}{#if sortable && app.sort.key === key}<span class="arrow" aria-hidden="true">{app.sort.desc ? "↓" : "↑"}</span>{/if}
    </button>
  </div>
{/snippet}

<div class="table" bind:this={host}>
  <div class="header" style:grid-template-columns={template}>
    {@render head("name", t("col.name"), false)}
    {#if columns.mode}{@render head("mode", t("col.mode"), true)}{/if}
    {#if columns.map}{@render head("map", t("col.map"), true)}{/if}
    {#if columns.ping}{@render head("ping", t("col.ping"), true, t("col.pingTooltip"))}{/if}
    {#if columns.round_time}{@render head("time", t("col.time"), true)}{/if}
    {#if columns.players}{@render head("players", t("col.players"), true)}{/if}
    <div class="cell last"></div>
  </div>

  <div class="scroll" data-scroll bind:this={scrollEl}>
    {#each rows as server, i (server.address)}
      {@const shift = shiftOf(i)}
      <div
        class="slot"
        class:moving={drag !== null}
        class:lifted={drag?.from === i}
        class:settling={drag?.from === i && drag.settling}
        style:transform={drag && shift !== 0 ? `translateY(${shift}px)` : undefined}
      >
        <ServerEntry
          {server}
          {raise}
          {columns}
          {template}
          grip={onreorder && rows.length > 1 ? (e) => begin(e, i) : undefined}
        />
      </div>
    {/each}
    {@render children?.()}
  </div>
</div>

<style>
  .table {
    display: flex;
    flex-direction: column;
    min-height: 0;
    min-width: 0;
    flex: 1;
    background: var(--decor);
  }
  .header {
    display: grid;
    padding: 0 10px 0 14px; /* the right side is the width of the scrollbar below */
    border-bottom: 1px solid var(--line);
  }
  .cell {
    padding: 8px 10px;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--sub);
  }
  .cell.center {
    text-align: center;
  }
  .hcell {
    padding: 0;
  }

  .hbtn {
    width: 100%;
    padding: 8px 10px;
    appearance: none;
    background: none;
    border: 0;
    font: inherit;
    letter-spacing: inherit;
    text-transform: inherit;
    text-align: left;
    cursor: pointer;
  }
  .hbtn.center {
    text-align: center;
  }
  .hbtn:disabled {
    cursor: default;
  }
  .hbtn:hover:not(:disabled) {
    color: var(--fg);
  }
  .arrow {
    margin-left: 4px;
    color: var(--accent);
  }
  .slot {
    position: relative;
  }
  .slot.moving {
    transition: transform 0.2s cubic-bezier(0.2, 0.8, 0.2, 1);
  }
  .slot.lifted {
    z-index: 2;
    transition: none;
    border-radius: var(--radius);
    background: var(--decor);
    box-shadow: 0 8px 24px rgb(0 0 0 / 0.35), 0 0 0 1px var(--line);
    cursor: grabbing;
  }
  .slot.lifted.settling {
    transition: transform 0.2s cubic-bezier(0.2, 0.8, 0.2, 1);
  }
  @media (prefers-reduced-motion: reduce) {
    .slot.moving,
    .slot.lifted.settling {
      transition: none;
    }
  }
  :global(body.reordering) {
    user-select: none;
    cursor: grabbing;
  }
  .scroll {
    flex: 1;
    min-height: 0;
    /* The last line of defence: the grid rows below stay in sync with this scrollbar. */
    overflow-x: auto;
    overflow-y: scroll;
    padding: 0 0 8px;
  }
</style>
