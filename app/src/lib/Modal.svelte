<script lang="ts">
  import { onDestroy, onMount, type Snippet } from "svelte";
  import { t } from "./i18n.svelte";
  import Icon from "./Icon.svelte";

  let {
    title,
    onclose,
    children,
  }: { title: string; onclose: () => void; children: Snippet } = $props();

  const FOCUSABLE =
    'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

  let backdrop = $state<HTMLElement>();
  let box = $state<HTMLElement>();

  /** Where the player was before the dialog opened, and the elements we made inert. */
  let returnFocus: HTMLElement | null = null;
  let inerted: HTMLElement[] = [];

  function focusables(): HTMLElement[] {
    if (!box) return [];
    return Array.from(box.querySelectorAll<HTMLElement>(FOCUSABLE)).filter(
      (el) => el.offsetParent !== null,
    );
  }

  /** The first control of the dialog body; the close button is only the last resort. */
  function firstControl(): HTMLElement | null {
    const body = box?.querySelector<HTMLElement>(".body");
    if (body) {
      const first = Array.from(body.querySelectorAll<HTMLElement>(FOCUSABLE)).find(
        (el) => el.offsetParent !== null,
      );
      if (first) return first;
    }
    return focusables()[0] ?? box ?? null;
  }

  /**
   * Everything around the dialog becomes unreachable for Tab and screen readers: every
   * sibling of every ancestor of the backdrop up to <body>. Undone on close.
   */
  function isolate(on: boolean) {
    if (!on) {
      for (const el of inerted) el.removeAttribute("inert");
      inerted = [];
      return;
    }
    let node: HTMLElement | null = backdrop ?? null;
    while (node && node.parentElement && node !== document.body) {
      const parent = node.parentElement;
      for (const child of Array.from(parent.children)) {
        if (child === node) continue;
        const el = child as HTMLElement;
        if (!el.hasAttribute("inert")) {
          el.setAttribute("inert", "");
          inerted.push(el);
        }
      }
      node = parent;
    }
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      onclose();
      return;
    }
    if (e.key !== "Tab" || !box) return;
    const items = focusables();
    if (items.length === 0) {
      e.preventDefault();
      box.focus();
      return;
    }
    const first = items[0];
    const last = items[items.length - 1];
    const active = document.activeElement as HTMLElement | null;
    if (!active || !box.contains(active)) {
      e.preventDefault();
      first.focus();
    } else if (e.shiftKey && active === first) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && active === last) {
      e.preventDefault();
      first.focus();
    }
  }

  onMount(() => {
    returnFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    isolate(true);
    firstControl()?.focus();
  });

  onDestroy(() => {
    isolate(false);
    if (returnFocus && document.contains(returnFocus)) returnFocus.focus();
  });
</script>

<svelte:window onkeydown={onKeydown} />

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="backdrop" bind:this={backdrop} onclick={(e) => e.target === e.currentTarget && onclose()}>
  <div
    class="box"
    bind:this={box}
    role="dialog"
    aria-modal="true"
    aria-label={title}
    tabindex="-1"
  >
    <header>
      <h2>{title}</h2>
      <button class="close" onclick={onclose} aria-label={t("dialog.close")}>
        <Icon name="close" size={18} />
      </button>
    </header>
    <div class="body">{@render children()}</div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 20;
    display: grid;
    place-items: center;
    background: var(--overlay);
    backdrop-filter: blur(2px);
    animation: backdrop-in 0.15s ease-out;
  }
  .box {
    width: min(480px, calc(100vw - 32px));
    max-height: calc(100vh - 48px);
    display: flex;
    flex-direction: column;
    background: var(--popup);
    border: 1px solid var(--line);
    border-radius: 12px;
    box-shadow: var(--shadow);
    animation: pop-in 0.16s ease-out;
    overflow: hidden;
    outline: none;
  }
  .box:focus-visible {
    outline: 2px solid var(--accent);
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 14px 18px;
    border-bottom: 1px solid var(--line);
  }
  .close {
    appearance: none;
    background: none;
    border: 0;
    padding: 5px;
    border-radius: var(--radius-sm);
    cursor: pointer;
    color: var(--sub);
    display: grid;
  }
  .close:hover {
    color: var(--fg);
    background: var(--tint-hover);
  }
  .body {
    padding: 18px;
    overflow: auto;
  }
</style>
