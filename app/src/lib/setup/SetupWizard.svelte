<script lang="ts">
  import { onDestroy, onMount, tick } from "svelte";
  import { errorText } from "../api";
  import { t } from "../i18n.svelte";
  import Icon from "../Icon.svelte";
  import { app } from "../store.svelte";
  import { SetupContext } from "./context.svelte";
  import { stepsFor, type StepId } from "./steps";
  import "./setup.css";
  import StepFinish from "./StepFinish.svelte";
  import StepGame from "./StepGame.svelte";
  import StepImport from "./StepImport.svelte";
  import StepLaunch from "./StepLaunch.svelte";
  import StepLook from "./StepLook.svelte";
  import StepPrivacy from "./StepPrivacy.svelte";
  import StepProfile from "./StepProfile.svelte";
  import StepServers from "./StepServers.svelte";
  import StepSystem from "./StepSystem.svelte";
  import StepTheme from "./StepTheme.svelte";
  import StepTools from "./StepTools.svelte";
  import StepWelcome from "./StepWelcome.svelte";

  const FOCUSABLE =
    'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

  const ctx = new SetupContext();
  ctx.load();

  const steps = $derived(stepsFor(ctx.officialFound));

  let current = $state<StepId>("welcome");
  /** Steps the player has reached; the rail lets them go back to these, not skip ahead. */
  let reached = $state<StepId[]>(["welcome"]);
  let busy = $state(false);

  const index = $derived(Math.max(0, steps.indexOf(current)));
  const last = $derived(index === steps.length - 1);
  const percent = $derived(Math.round(((index + 1) / steps.length) * 100));

  let root = $state<HTMLElement>();
  let scroller = $state<HTMLElement>();
  let heading = $state<HTMLElement>();
  let inerted: HTMLElement[] = [];
  let returnFocus: HTMLElement | null = null;

  async function go(id: StepId) {
    current = id;
    if (!reached.includes(id)) reached = [...reached, id];
    await tick();
    scroller?.scrollTo({ top: 0 });
    heading?.focus({ preventScroll: true });
  }

  function back() {
    if (index > 0) void go(steps[index - 1]);
  }

  async function next() {
    if (!last) {
      void go(steps[index + 1]);
      return;
    }
    busy = true;
    try {
      await app.finishSetup();
    } catch (e) {
      app.say(errorText(e));
    } finally {
      busy = false;
    }
  }

  function close() {
    void app.finishSetup();
  }

  function focusables(): HTMLElement[] {
    if (!root) return [];
    return Array.from(root.querySelectorAll<HTMLElement>(FOCUSABLE)).filter(
      (el) => el.offsetParent !== null,
    );
  }

  /**
   * Captured before anything else sees the key: the first run cannot be dismissed with Escape,
   * and Tab never leaves the wizard. A drop-down that is open keeps its own Escape.
   */
  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      if (e.target instanceof HTMLSelectElement) return;
      e.preventDefault();
      e.stopPropagation();
      if (app.setupRerun) close();
      return;
    }
    if (e.key !== "Tab" || !root) return;
    const items = focusables();
    if (items.length === 0) {
      e.preventDefault();
      root.focus();
      return;
    }
    const first = items[0];
    const lastEl = items[items.length - 1];
    const active = document.activeElement as HTMLElement | null;
    if (!active || !root.contains(active)) {
      e.preventDefault();
      first.focus();
    } else if (e.shiftKey && active === first) {
      e.preventDefault();
      lastEl.focus();
    } else if (!e.shiftKey && active === lastEl) {
      e.preventDefault();
      first.focus();
    }
  }

  onMount(() => {
    returnFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    // Only the launcher itself is shut off; the toast stays readable above the wizard.
    const shell = document.querySelector<HTMLElement>(".shell");
    if (shell && !shell.hasAttribute("inert")) {
      shell.setAttribute("inert", "");
      inerted.push(shell);
    }
    window.addEventListener("keydown", onkeydown, true);
    heading?.focus({ preventScroll: true });
  });

  onDestroy(() => {
    window.removeEventListener("keydown", onkeydown, true);
    for (const el of inerted) el.removeAttribute("inert");
    inerted = [];
    if (returnFocus && document.contains(returnFocus)) returnFocus.focus();
  });
</script>

<div
  class="setup"
  bind:this={root}
  role="dialog"
  aria-modal="true"
  aria-labelledby="su-title"
  tabindex="-1"
>
  <nav class="rail" aria-label={t("setup.railLabel")}>
    <div class="rail-head">
      <strong>{app.setupRerun ? t("setup.railTitleAgain") : t("setup.railTitle")}</strong>
      <span class="sub">{t("setup.stepOf", { n: index + 1, total: steps.length })}</span>
    </div>
    <ol>
      {#each steps as id, i (id)}
        {@const seen = reached.includes(id)}
        {@const done = seen && i < index}
        <li>
          <button
            class="stop"
            class:on={id === current}
            class:done
            disabled={!seen}
            aria-current={id === current ? "step" : undefined}
            onclick={() => void go(id)}
          >
            <span class="dot" aria-hidden="true">
              {#if done}<Icon name="check" size={14} />{:else}{i + 1}{/if}
            </span>
            <span class="label">{t(`setup.${id}.nav`)}</span>
          </button>
        </li>
      {/each}
    </ol>
  </nav>

  <section class="main">
    <div class="compact" aria-hidden="true">
      <span>{t("setup.stepOf", { n: index + 1, total: steps.length })}</span>
      <span class="bar"><span class="fill" style:width="{percent}%"></span></span>
    </div>

    <div class="scroll" bind:this={scroller}>
      <header>
        <div class="titles">
          <h2 id="su-title" tabindex="-1" bind:this={heading}>{t(`setup.${current}.title`)}</h2>
          <p class="intro">{t(`setup.${current}.intro`)}</p>
        </div>
        {#if app.setupRerun}
          <button class="btn icon ghost" onclick={close} aria-label={t("setup.close")}>
            <Icon name="close" size={18} />
          </button>
        {/if}
      </header>

      {#key current}
        <div class="step">
          {#if current === "welcome"}
            <StepWelcome />
          {:else if current === "theme"}
            <StepTheme />
          {:else if current === "look"}
            <StepLook />
          {:else if current === "profile"}
            <StepProfile {ctx} />
          {:else if current === "game"}
            <StepGame />
          {:else if current === "launch"}
            <StepLaunch />
          {:else if current === "servers"}
            <StepServers />
          {:else if current === "import"}
            <StepImport {ctx} />
          {:else if current === "system"}
            <StepSystem />
          {:else if current === "privacy"}
            <StepPrivacy />
          {:else if current === "tools"}
            <StepTools />
          {:else}
            <StepFinish />
          {/if}
        </div>
      {/key}
    </div>

    <footer>
      <button class="btn" onclick={back} disabled={index === 0 || busy}>
        <span class="turn" aria-hidden="true"><Icon name="chevron" size={16} /></span>
        {t("setup.back")}
      </button>
      {#if !app.setupRerun}
        <p class="mandatory sub">{t("setup.mandatory")}</p>
      {/if}
      <button class="btn primary" onclick={() => void next()} disabled={busy}>
        {last ? t("setup.finish.button") : t("setup.next")}
        {#if !last}<span class="turn fwd" aria-hidden="true"><Icon name="chevron" size={16} /></span>{/if}
      </button>
    </footer>
  </section>
</div>

<style>
  .setup {
    position: fixed;
    inset: 0;
    z-index: 24;
    display: grid;
    grid-template-columns: 232px minmax(0, 1fr);
    background: var(--bg-fill);
    color: var(--fg);
    outline: none;
    animation: setup-in 0.2s ease-out;
  }
  @keyframes setup-in {
    from {
      opacity: 0;
    }
  }

  .rail {
    display: flex;
    flex-direction: column;
    gap: 18px;
    min-height: 0;
    padding: 22px 14px 18px;
    background: var(--tint);
    border-right: 1px solid var(--line);
    overflow-y: auto;
  }
  .rail-head {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 0 10px;
  }
  .rail-head strong {
    font-size: 16px;
    font-weight: 600;
  }
  .rail-head .sub {
    font-size: 13px;
  }
  ol {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .stop {
    appearance: none;
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    padding: 7px 10px;
    border: 0;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--sub);
    font: inherit;
    font-size: 14px;
    text-align: left;
    cursor: pointer;
  }
  .stop:disabled {
    cursor: default;
    opacity: 0.55;
  }
  .stop:not(:disabled):hover {
    background: var(--tint-hover);
    color: var(--fg);
  }
  .stop.on {
    background: var(--accent-soft);
    color: var(--fg);
    font-weight: 600;
  }
  .stop:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }
  .dot {
    flex: none;
    display: grid;
    place-items: center;
    width: 24px;
    height: 24px;
    border-radius: 50%;
    border: 1px solid var(--line);
    font-size: 12px;
    font-variant-numeric: tabular-nums;
  }
  .stop.on .dot {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--on-accent);
  }
  .stop.done .dot {
    border-color: var(--accent);
    color: var(--accent);
  }
  .label {
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .mandatory {
    flex: 1;
    align-self: center;
    margin: 0;
    font-size: 12.5px;
    line-height: 1.35;
  }

  .main {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
  }
  .compact {
    display: none;
    align-items: center;
    gap: 12px;
    padding: 10px 18px;
    border-bottom: 1px solid var(--line);
    font-size: 13px;
    color: var(--sub);
    white-space: nowrap;
  }
  .bar {
    flex: 1;
    height: 4px;
    border-radius: 2px;
    background: var(--tint-hover);
    overflow: hidden;
  }
  .fill {
    display: block;
    height: 100%;
    background: var(--accent);
    transition: width 0.2s ease-out;
  }
  .scroll {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 28px 36px 24px;
  }
  header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 16px;
    max-width: 760px;
    margin-bottom: 22px;
  }
  .titles {
    min-width: 0;
  }
  h2 {
    margin: 0 0 6px;
    font-size: 24px;
    font-weight: 600;
    line-height: 1.2;
    outline: none;
  }
  .intro {
    margin: 0;
    font-size: 15px;
    line-height: 1.5;
    color: var(--sub);
  }
  .step {
    max-width: 760px;
    animation: step-in 0.18s ease-out;
  }
  @keyframes step-in {
    from {
      opacity: 0;
      transform: translateY(6px);
    }
  }
  footer {
    display: flex;
    justify-content: space-between;
    gap: 12px;
    padding: 12px 36px;
    background: var(--popup);
    border-top: 1px solid var(--line);
  }
  .turn {
    display: inline-grid;
    rotate: 90deg;
  }
  .turn.fwd {
    rotate: -90deg;
  }
  footer :global(.btn) {
    gap: 4px;
  }

  @media (max-width: 860px) {
    .setup {
      grid-template-columns: minmax(0, 1fr);
    }
    .rail {
      display: none;
    }
    .compact {
      display: flex;
    }
    .scroll {
      padding: 20px 18px 18px;
    }
    footer {
      padding: 10px 18px;
    }
    .mandatory {
      display: none;
    }
    h2 {
      font-size: 21px;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .setup,
    .step {
      animation: none;
    }
    .fill {
      transition: none;
    }
  }
</style>
