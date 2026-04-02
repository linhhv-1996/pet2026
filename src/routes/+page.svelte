<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import * as PIXI from 'pixi.js';
  import { PetSprite } from '../PetSprite';
  import { ChatBubble } from '../ChatBubble';

  // ── Constants ──────────────────────────────────────────
  const DISPLAY     = 256;
  const SCALE       = DISPLAY / 48;
  const BOTTOM_GAP  = 16 * SCALE;
  const WALK_SPEED  = 2.2;
  const GRAVITY     = 0.9;
  const GLIDE_G     = 0.3;
  const JUMP_FORCE  = -14;

  function getGroundY() {
    return window.innerHeight - DISPLAY + BOTTOM_GAP;
  }

  // ── State machine types ────────────────────────────────
  type PetState =
    | 'Idle' | 'Idle2' | 'Idle(Bug)'
    | 'Sit'  | 'Sit(Idle)' | 'Stand'
    | 'Shock'
    | 'Run'
    | 'Jump' | 'Fall' | 'Landing' | 'Glide'
    | 'Sleep(Start)' | 'Sleep' | 'Awake'
    | 'Hurt' | 'Death'
    | 'Dragging';

  const SLEEP_INTERRUPTIBLE = new Set<PetState>([
    'Idle', 'Idle2', 'Idle(Bug)', 'Sit(Idle)', 'Run', 'Shock',
  ]);
  const HURT_INTERRUPTIBLE = new Set<PetState>([
    'Idle', 'Idle2', 'Idle(Bug)', 'Sit', 'Sit(Idle)', 'Stand', 'Shock', 'Run',
  ]);
  const IDLE_GROUP = new Set<PetState>([
    'Idle', 'Idle2', 'Idle(Bug)', 'Sit(Idle)',
  ]);

  // ── Pet instance ───────────────────────────────────────
  const pet = new PetSprite(
    '/Frog_4.json',
    '/Frog_4.png',
    {
      'Idle':         7,
      'Idle2':        7,
      'Idle(Bug)':    8,
      'Run':          10,
      'Sleep(Start)': 6,
      'Sleep':        4,
      'Awake':        6,
      'Jump':         9,
      'Fall':         8,
      'Landing':      10,
      'Hurt':         12,
      'Death':        8,
      'Sit':          8,
      'Sit(Idle)':    5,
      'Stand':        8,
      'Shock':        10,
      'Glide':        8,
    }
  );

  // ── Runtime state ──────────────────────────────────────
  interface WinInfo { title: string; x: number; y: number; width: number; height: number; }

  let state: PetState = 'Idle';
  let petX = 15;
  let petY = 0;
  let vx   = WALK_SPEED;
  let vy   = 0;

  let clickCount    = 0;
  let clickTimer:    ReturnType<typeof setTimeout>;
  let behaviorTimer: ReturnType<typeof setTimeout>;
  let bubbleTimer:   ReturnType<typeof setTimeout>;

  let bubble: ChatBubble;
  let idleStartTime = Date.now();

  let openWindows: WinInfo[] = [];
  let pixiApp:     PIXI.Application;
  let canvasEl:    HTMLCanvasElement;
  let lastRectUpdate = 0;

  // ── Helpers ────────────────────────────────────────────
  function isIdling()   { return IDLE_GROUP.has(state); }
  function isSleeping() { return state === 'Sleep(Start)' || state === 'Sleep' || state === 'Awake'; }
  function isDragging() { return state === 'Dragging'; }
  function clearBehavior() { clearTimeout(behaviorTimer); }

  function maybeUpdateRect() {
    if (isDragging()) return;
    const now = performance.now();
    if (now - lastRectUpdate > 32) {
      lastRectUpdate = now;
      invoke('update_pet_rect', { x: petX, y: petY, w: DISPLAY, h: DISPLAY });
    }
  }

  // ── Bubble helpers ─────────────────────────────────────

  function showMood(mood: string, durationMs = 2800) {
    const centerX = pet.container.x + DISPLAY / 2;
    const headY   = pet.getHeadY();
    bubble?.showMood(mood, centerX, headY, durationMs);
  }

  function scheduleBubble() {
    clearTimeout(bubbleTimer);
    const delay = 8000 + Math.random() * 12000;

    bubbleTimer = setTimeout(() => {
      if (!bubble) { scheduleBubble(); return; }

      const idleSecs = (Date.now() - idleStartTime) / 1000;

      if (isSleeping()) {
        showMood('sleep');
      } else if (isIdling()) {
        if (idleSecs > 30) {
          showMood(Math.random() < 0.5 ? 'hungry' : 'bored');
        } else {
          showMood(Math.random() < 0.5 ? 'idle' : 'happy');
        }
      }
      // đang chạy/nhảy → bỏ qua lần này, schedule lại thôi

      scheduleBubble();
    }, delay);
  }

  // ── Core state transitions ─────────────────────────────

  function enterState(s: PetState) {
    if (IDLE_GROUP.has(s) && !IDLE_GROUP.has(state)) {
      idleStartTime = Date.now();
    }
    state = s;
    pet.play(s);
  }

  function playOnce(s: PetState, onDone: () => void) {
    state = s;
    pet.playOnce(s, onDone);
  }

  function land() {
    playOnce('Landing', () => {
      enterState('Idle');
      scheduleNextBehavior();
    });
  }

  // ── Sleep transitions ──────────────────────────────────

  function enterSleep() {
    clearBehavior();
    playOnce('Sleep(Start)', () => enterState('Sleep'));
  }

  function exitSleep() {
    bubble?.hide();
    playOnce('Awake', () => {
      enterState('Idle');
      scheduleNextBehavior();
    });
  }

  // ── Behavior scheduler ─────────────────────────────────

  function scheduleNextBehavior() {
    clearBehavior();
    if (!isIdling()) return;

    const rand = Math.random();

    if (rand < 0.10) {
      behaviorTimer = setTimeout(() => {
        if (!isIdling()) return;
        playOnce('Idle(Bug)', () => {
          enterState('Idle');
          scheduleNextBehavior();
        });
      }, 4000 + Math.random() * 6000);

    } else if (rand < 0.22) {
      behaviorTimer = setTimeout(() => {
        if (!isIdling()) return;
        const next: PetState = state === 'Idle' ? 'Idle2' : 'Idle';
        enterState(next);
        scheduleNextBehavior();
      }, 3000 + Math.random() * 4000);

    } else if (rand < 0.36) {
      behaviorTimer = setTimeout(() => {
        if (!isIdling()) return;
        playOnce('Sit', () => {
          enterState('Sit(Idle)');
          behaviorTimer = setTimeout(() => {
            if (!isIdling()) return;
            playOnce('Stand', () => {
              enterState('Idle');
              scheduleNextBehavior();
            });
          }, 2000 + Math.random() * 3000);
        });
      }, 2000 + Math.random() * 3000);

    } else if (rand < 0.48) {
      behaviorTimer = setTimeout(() => {
        if (!isIdling()) return;
        playOnce('Shock', () => {
          enterState('Idle');
          scheduleNextBehavior();
        });
      }, 1500 + Math.random() * 2000);

    } else {
      behaviorTimer = setTimeout(() => {
        if (!isIdling()) return;
        pet.facingLeft = Math.random() < 0.5;
        vx = pet.facingLeft ? -WALK_SPEED : WALK_SPEED;
        enterState('Run');
        behaviorTimer = setTimeout(() => {
          if (state !== 'Run') return;
          enterState('Idle');
          scheduleNextBehavior();
        }, 1500 + Math.random() * 2500);
      }, 2000 + Math.random() * 3000);
    }
  }

  // ── Physics ────────────────────────────────────────────

  function updatePhysics() {
    const gY = getGroundY();

    if (state === 'Run') {
      petX += vx;
      const maxX = window.innerWidth - DISPLAY;
      if (petX <= 0)    { petX = 0;    vx =  Math.abs(vx); pet.facingLeft = false; }
      if (petX >= maxX) { petX = maxX; vx = -Math.abs(vx); pet.facingLeft = true;  }

    } else if (state === 'Jump' || state === 'Fall') {
      vy += GRAVITY;
      petY += vy;
      if (vy > 0 && state === 'Jump') enterState('Fall');
      if (petY >= gY) { petY = gY; vy = 0; land(); }

    } else if (state === 'Glide') {
      vy += GLIDE_G;
      petY += vy;
      if (petY >= gY) { petY = gY; vy = 0; land(); }
    }
  }

  // ── Game loop ──────────────────────────────────────────

  function gameTick() {
    if (!isDragging()) updatePhysics();
    pet.setPosition(petX, petY);
    bubble?.updatePosition(pet.container.x + DISPLAY / 2, pet.getHeadY());
    maybeUpdateRect();
  }

  // ── Click handling ─────────────────────────────────────

  function handlePetClick() {
    if (isDragging()) return;

    if (isSleeping()) { exitSleep(); return; }

    if (['Jump', 'Fall', 'Landing', 'Glide', 'Death'].includes(state)) return;

    clickCount++;
    clearTimeout(clickTimer);
    clickTimer = setTimeout(() => { clickCount = 0; }, 800);

    if (clickCount >= 5) {
      clickCount = 0;
      clearBehavior();
      bubble?.hide();
      playOnce('Death', () => {
        setTimeout(() => { enterState('Idle'); scheduleNextBehavior(); }, 1500);
      });
      return;
    }

    if (!HURT_INTERRUPTIBLE.has(state)) return;

    showMood('scared', 1500);

    clearBehavior();
    playOnce('Hurt', () => {
      vy = JUMP_FORCE;
      vx = (Math.random() < 0.5 ? 1 : -1) * (2 + Math.random() * 2);
      pet.facingLeft = vx < 0;
      enterState('Jump');
    });
  }

  // ── Lifecycle ──────────────────────────────────────────

  onMount(async () => {
    petY = getGroundY();

    pixiApp = new PIXI.Application();
    await pixiApp.init({
      width:           window.innerWidth,
      height:          window.innerHeight,
      backgroundAlpha: 0,
      antialias:       false,
      resolution:      window.devicePixelRatio || 1,
      autoDensity:     true,
    });

    PIXI.TextureSource.defaultOptions.scaleMode = 'nearest';

    canvasEl = pixiApp.canvas as HTMLCanvasElement;
    Object.assign(canvasEl.style, {
      position:       'fixed',
      top:            '0',
      left:           '0',
      width:          '100vw',
      height:         '100vh',
      imageRendering: 'pixelated',
      pointerEvents:  'none',
    });
    document.body.appendChild(canvasEl);

    await pet.load(pixiApp, DISPLAY);

    // Bubble load SAU pet để render on top
    bubble = new ChatBubble(pixiApp.stage);
    await bubble.load();

    invoke('update_pet_rect', { x: petX, y: petY, w: DISPLAY, h: DISPLAY });
    await invoke('request_accessibility');

    pixiApp.ticker.add(gameTick);
    enterState('Idle');
    scheduleNextBehavior();
    scheduleBubble();

    // ── Callbacks từ Rust ────────────────────────────────

    (window as any).__onPetClicked = () => handlePetClick();

    (window as any).__onPetDragStart = () => {
      clearBehavior();
      state = 'Dragging';
      pet.play('Shock');
      showMood('scared', 1200);
    };

    (window as any).__onPetDrag = (x: number, y: number) => {
      petX = x;
      petY = y;
    };

    (window as any).__onPetDragEnd = () => {
      bubble?.hide();
      const gY = getGroundY();
      if (petY < gY - 5) {
        vy = 2;
        enterState('Glide');
      } else {
        petY = gY;
        land();
      }
      invoke('update_pet_rect', { x: petX, y: petY, w: DISPLAY, h: DISPLAY });
    };

    (window as any).__onUserAFK = () => {
      if (isDragging() || isSleeping()) return;
      if (!SLEEP_INTERRUPTIBLE.has(state)) return;
      enterSleep();
    };

    (window as any).__onUserActive = () => {
      if (!isSleeping()) return;
      exitSleep();
    };

    (window as any).__onWindowsUpdated = (wins: WinInfo[]) => {
      openWindows = wins;
    };
  });

  onDestroy(() => {
    clearBehavior();
    clearTimeout(clickTimer);
    clearTimeout(bubbleTimer);
    bubble?.destroy();
    pet.destroy();
    canvasEl?.remove();
    pixiApp?.destroy(true);
    delete (window as any).__onPetClicked;
    delete (window as any).__onPetDragStart;
    delete (window as any).__onPetDrag;
    delete (window as any).__onPetDragEnd;
    delete (window as any).__onUserAFK;
    delete (window as any).__onUserActive;
    delete (window as any).__onWindowsUpdated;
  });
</script>

<div class="overlay"></div>

<style>
  :global(body) {
    margin: 0;
    background: transparent;
    overflow: hidden;
    user-select: none;
  }
  .overlay {
    position: fixed;
    inset: 0;
    pointer-events: none;
  }
</style>
