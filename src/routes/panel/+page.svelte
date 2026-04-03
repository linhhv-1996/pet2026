<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";

  const FROGS = ["Frog_1", "Frog_2", "Frog_3", "Frog_4"] as const;
  type FrogId = (typeof FROGS)[number];

  const PRESETS = [25, 50, 90];

  let selectedFrog: FrogId = "Frog_1";
  let focusMins = 25;
  let customInput = "";
  let isCustom = false;
  let petVisible = true;
  let panel: HTMLDivElement;

  onMount(() => {
    (async () => {
      try {
        const [frog, mins] = await invoke<[string, number]>("get_settings");
        selectedFrog = frog as FrogId;
        focusMins    = mins;
        if (!PRESETS.includes(mins)) {
          isCustom = true;
          customInput = String(mins);
        }
      } catch {}
    })();
  });

  async function pickFrog(frog: FrogId) {
    selectedFrog = frog;
    await invoke("set_frog", { frog });
  }

  async function pickPreset(mins: number) {
    isCustom = false;
    customInput = "";
    focusMins = mins;
    await invoke("set_focus_mins", { mins });
  }

  async function applyCustom() {
    const v = parseInt(customInput);
    if (!v || v < 1 || v > 480) return;
    focusMins = v;
    await invoke("set_focus_mins", { mins: v });
  }

  function onCustomKey(e: KeyboardEvent) {
    if (e.key === "Enter") applyCustom();
  }

  async function togglePet() {
    petVisible = !petVisible;
    await invoke("toggle_pet", { visible: petVisible });
  }

  function frogLabel(f: FrogId) { return f.replace("_", " "); }
</script>

<div class="panel" bind:this={panel}>

  <div class="header">
    <span class="header-title">🐸 Pet Settings</span>
  </div>

  <div class="section row">
    <span class="row-label">Show pet on screen</span>
    <button class="toggle {petVisible ? 'on' : ''}" on:click={togglePet}>
      <div class="thumb"></div>
    </button>
  </div>

  <div class="section">
    <div class="section-label">Choose your frog</div>
    <div class="frog-grid">
      {#each FROGS as frog}
        <button
          class="frog-card {selectedFrog === frog ? 'active' : ''}"
          on:click={() => pickFrog(frog)}
        >
          <div class="frog-img" style="background-image:url('/{frog}.png')"></div>
          <span class="frog-name">{frogLabel(frog)}</span>
          {#if selectedFrog === frog}<span class="check">✓</span>{/if}
        </button>
      {/each}
    </div>
  </div>

  <div class="section">
    <div class="section-label">Focus duration</div>
 
    <div class="tabs">
      {#each PRESETS as p}
        <button
          class="tab {!isCustom && focusMins === p ? 'tab-active' : ''}"
          on:click={() => pickPreset(p)}
        >{p} min</button>
      {/each}
    </div>

    <div class="custom-row">
      <input
        class="custom-input {isCustom ? 'custom-active' : ''}"
        type="number"
        min="1"
        max="480"
        placeholder="Custom (e.g. 45 min)"
        bind:value={customInput}
        on:focus={() => { isCustom = true; }}
        on:keydown={onCustomKey}
        on:blur={applyCustom}
      />
    </div>

    <p class="hint">
      Frog stays awake for 
      <strong>{focusMins} min</strong> with you 🐸
    </p>
  </div>

  <div class="footer">
    <button class="footer-btn danger" on:click={() => invoke("quit_app")}>Quit</button>
  </div>

</div>

<style>
  :global(html), :global(body) {
    margin: 0;
    padding: 0;
    background: transparent !important;
    overflow: hidden;
    font-family: -apple-system, BlinkMacSystemFont, "Helvetica Neue", sans-serif;
    user-select: none;
    -webkit-user-select: none;
  }
  :global(*) { box-sizing: border-box; }

  button {
    appearance: none;
    background: transparent;
    border: none;
    padding: 0;
    margin: 0;
    font-family: inherit;
    color: inherit;
    text-align: left;
  }

  .panel {
    width: 300px;
    background: rgba(30, 30, 32, 0.96);
    backdrop-filter: blur(40px) saturate(200%);
    -webkit-backdrop-filter: blur(40px) saturate(200%);
    border: 1px solid rgba(255,255,255,0.12);
    border-radius: 14px;
    color: #f2f2f7;
    overflow: hidden;
  }

  /* Header */
  .header {
    display: flex;
    align-items: center;
    padding: 13px 14px 11px;
    border-bottom: 1px solid rgba(255,255,255,0.08);
    -webkit-app-region: drag;
    cursor: default;
  }
  .header-title { font-size: 13px;
    font-weight: 700;
    color: rgba(255,255,255,0.85); }

  /* Section */
  .section {
    padding: 12px 14px;
    border-bottom: 1px solid rgba(255,255,255,0.06);
    -webkit-app-region: no-drag;
  }
  .section.row {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .section-label {
    font-size: 11px;
    font-weight: 600;
    color: rgba(255,255,255,0.35);
    margin-bottom: 10px;
  }
  .row-label { font-size: 12px; font-weight: 500; color: rgba(255,255,255,0.65);
  }

  /* Toggle */
  .toggle {
    display: block;
    width: 36px; height: 20px;
    background: rgba(255,255,255,0.12);
    border-radius: 10px;
    cursor: pointer;
    position: relative;
    transition: background 0.2s;
    flex-shrink: 0;
  }
  .toggle.on { background: #34d399; }
  .thumb {
    position: absolute;
    top: 2px; left: 2px;
    width: 16px; height: 16px;
    background: #fff;
    border-radius: 50%;
    transition: transform 0.2s;
    box-shadow: 0 1px 3px rgba(0,0,0,0.3);
  }
  .toggle.on .thumb { transform: translateX(16px); }

  /* Frog Grid */
  .frog-grid { display: grid; grid-template-columns: repeat(4, 1fr); gap: 7px; }
  .frog-card {
    display: flex; flex-direction: column; align-items: center; gap: 4px;
    padding: 7px 2px 6px;
    border-radius: 10px;
    border: 1.5px solid transparent; background: rgba(255,255,255,0.05);
    cursor: pointer; transition: background 0.12s, border-color 0.12s, transform 0.1s;
    position: relative;
  }
  .frog-card:hover { background: rgba(255,255,255,0.09); transform: translateY(-1px); }
  .frog-card.active { border-color: #34d399; background: rgba(52,211,153,0.1); }
  .frog-img {
    width: 48px; height: 48px;
    background-position: 0 0; background-repeat: no-repeat;
    background-size: auto; image-rendering: pixelated;
    border-radius: 4px;
  }
  .frog-name { font-size: 9.5px; color: rgba(255,255,255,0.38); font-weight: 500; }
  .frog-card.active .frog-name { color: #34d399; }
  .check { position: absolute; top: 3px; right: 5px; font-size: 9px; color: #34d399; font-weight: 800; }

  /* Preset tabs */
  .tabs { display: flex; gap: 5px; margin-bottom: 8px; }
  .tab {
    display: block; flex: 1; padding: 7px 4px; border-radius: 8px;
    border: 1.5px solid transparent; background: rgba(255,255,255,0.05);
    color: rgba(255,255,255,0.5); font-size: 12px; font-weight: 600;
    text-align: center; cursor: pointer; font-family: inherit;
    transition: background 0.12s, border-color 0.12s, color 0.12s;
  }
  .tab:hover { background: rgba(255,255,255,0.09); color: rgba(255,255,255,0.8); }
  .tab-active { border-color: #60a5fa; background: rgba(96,165,250,0.12); color: #93c5fd; }

  /* Custom input */
  .custom-row { margin-bottom: 10px; }
  .custom-input {
    width: 100%;
    padding: 8px 11px;
    border-radius: 8px;
    border: 1.5px solid rgba(255,255,255,0.1);
    background: rgba(255,255,255,0.06);
    color: #f2f2f7;
    font-size: 13px;
    font-family: inherit;
    outline: none;
    -webkit-app-region: no-drag;
    transition: border-color 0.15s;
  }
  .custom-input:focus,
  .custom-input.custom-active { border-color: #60a5fa; }
  .custom-input::placeholder { color: rgba(255,255,255,0.2); }
  .custom-input::-webkit-inner-spin-button,
  .custom-input::-webkit-outer-spin-button { -webkit-appearance: none; }

  .hint { font-size: 11.5px; color: rgba(255,255,255,0.32); line-height: 1.5; margin: 0; }
  .hint strong { color: rgba(255,255,255,0.58); }

  /* Footer */
  .footer { -webkit-app-region: no-drag; }
  .footer-btn {
    display: block; width: 100%;
    padding: 11px 0; text-align: center;
    font-size: 12px; font-weight: 500; cursor: pointer;
    color: rgba(255,255,255,0.3);
    transition: background 0.12s, color 0.12s;
  }
  .footer-btn.danger:hover { background: rgba(239,68,68,0.1); color: #f87171; }
</style>
