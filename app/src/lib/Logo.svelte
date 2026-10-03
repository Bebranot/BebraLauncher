<script lang="ts"></script>

<!-- The launcher's wordmark: a rounded mark in the accent colour and two words. -->
<div class="logo" role="img" aria-label="BebraLauncher">
  <span class="mark">B</span>
  <span class="word"><b>Bebra</b><span class="sub tail">Launcher</span></span>
</div>

<style>
  .logo {
    display: flex;
    align-items: center;
    gap: 9px;
    white-space: nowrap;
  }
  .mark {
    display: grid;
    place-items: center;
    width: 28px;
    height: 28px;
    border-radius: 8px;
    background: linear-gradient(
      135deg,
      var(--accent),
      color-mix(in srgb, var(--accent) 62%, var(--bg))
    );
    color: var(--on-accent);
    font-size: 16px;
    font-weight: 700;
    line-height: 1;
  }
  .word {
    display: flex;
    align-items: baseline;
    gap: 5px;
    font-size: 15px;
  }
  b {
    font-weight: 600;
  }
  /* The window is at its narrowest: the mark and one word are enough. */
  @media (max-width: 900px) {
    .tail {
      display: none;
    }
  }
</style>
