<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';

  const DISPLAY = 96;

  const I1 = 'Alien_IDLE_1.png';
  const I2 = 'Alien_IDLE_2.png';
  const I3 = 'Alien_IDLE_3.png';
  const I4 = 'Alien_IDLE_4.png';
  const B1 = 'Alien_BLINK_1.png';
  const B2 = 'Alien_BLINK_2.png';

  const ANIMS: Record<string, string[]> = {
    idle: [
      I1, I2, I3, I4,
      I1, I2, I3, I4,
      I1, I2, I3, I4,
      B1, B2, B2, B1,
      I1,
    ],
    run:  ['Alien_RUN_1.png', 'Alien_RUN_2.png', 'Alien_RUN_3.png', 'Alien_RUN_4.png'],
    jump: [
      'Alien_JUMP.png', 'Alien_JUMP.png'
    ],
    fall: ['Alien_FALL_1.png', 'Alien_FALL_2.png'],
    hit:  ['Alien_HIT_1.png', 'Alien_HIT_2.png', 'Alien_HIT_1.png', 'Alien_HIT_2.png'],
    dead: ['Alien_DEAD.png'],
    // Drag: dùng JUMP frame (đang bị nhấc lên)
    drag: ['Alien_JUMP.png'],
  };

  const ANIM_FPS: Record<string, number> = {
    idle: 12, run: 12, jump: 12, fall: 12, hit: 12, dead: 2, drag: 2,
  };

  const images: Record<string, HTMLImageElement> = {};
  const ALL_FILES = [...new Set(Object.values(ANIMS).flat())];

  interface WinInfo { title: string; x: number; y: number; width: number; height: number; }

  let canvas: HTMLCanvasElement;
  let ctx: CanvasRenderingContext2D;

  const WALK_SPEED   = 2.2;
  const FALL_GRAVITY = 5.9;
  const JUMP_FORCE   = -35;

  let petX = 200;
  let petY = 0;
  let vx = WALK_SPEED;
  let vy = 0;
  let facingLeft = false;
  let isDragging = false;

  type PetState = 'idle' | 'run' | 'jump' | 'fall' | 'hit' | 'dead' | 'drag';
  let state: PetState = 'idle';
  let prevState: PetState = 'idle';
  let stateBeforeDrag: PetState = 'idle';

  let animFrameIndex = 0;
  let rafId: number;
  let lastFrameTime = 0;
  let behaviorTimeout: ReturnType<typeof setTimeout>;

  let openWindows: WinInfo[] = [];
  let targetWin: WinInfo | null = null;

  // Throttle update_pet_rect ~30fps
  let lastRectUpdate = 0;
  function maybeUpdateRect() {
    if (isDragging) return; // Rust đã biết vị trí qua drag, không cần update ngược lại
    const now = performance.now();
    if (now - lastRectUpdate > 32) {
      lastRectUpdate = now;
      invoke('update_pet_rect', { x: petX, y: petY, w: DISPLAY, h: DISPLAY });
    }
  }

  onMount(async () => {
    petY = window.innerHeight - DISPLAY;
    ctx = canvas.getContext('2d')!;
    ctx.imageSmoothingEnabled = false;

    await preloadAll();

    invoke('update_pet_rect', { x: petX, y: petY, w: DISPLAY, h: DISPLAY });
    await invoke('request_accessibility');

    // ── Callbacks từ Rust qua eval() ──────────────────

    (window as any).__onPetClicked = () => {
      handlePetClick();
    };

    // Rust báo bắt đầu drag
    (window as any).__onPetDragStart = () => {
      isDragging = true;
      stateBeforeDrag = state;
      clearTimeout(behaviorTimeout);
      enterState('drag');
    };

    // Rust gửi tọa độ mới liên tục khi drag
    (window as any).__onPetDrag = (x: number, y: number) => {
      petX = x;
      petY = y;
    };

    // Rust báo thả ra
    (window as any).__onPetDragEnd = () => {
      isDragging = false;
      // Cập nhật rect lần cuối với vị trí mới
      invoke('update_pet_rect', { x: petX, y: petY, w: DISPLAY, h: DISPLAY });

      // Thả ra → rơi xuống đất (trừ khi đang ở gần đất rồi)
      const gY = window.innerHeight - DISPLAY;
      if (petY < gY - 5) {
        vy = 2; // rơi nhẹ
        enterState('fall');
      } else {
        petY = gY;
        enterState('idle');
        scheduleNextBehavior();
      }
    };

    (window as any).__onWindowsUpdated = (wins: WinInfo[]) => {
      openWindows = wins;
    };

    enterState('idle');
    scheduleNextBehavior();
    rafId = requestAnimationFrame(loop);
  });

  onDestroy(() => {
    cancelAnimationFrame(rafId);
    clearTimeout(behaviorTimeout);
    delete (window as any).__onPetClicked;
    delete (window as any).__onPetDragStart;
    delete (window as any).__onPetDrag;
    delete (window as any).__onPetDragEnd;
    delete (window as any).__onWindowsUpdated;
  });

  function preloadAll(): Promise<void> {
    return new Promise((resolve) => {
      let loaded = 0;
      for (const file of ALL_FILES) {
        const img = new Image();
        img.src = `/${file}`;
        img.onload = () => { if (++loaded === ALL_FILES.length) resolve(); };
        img.onerror = () => { if (++loaded === ALL_FILES.length) resolve(); };
        images[file] = img;
      }
    });
  }

  // ── ANIMATION LOOP ────────────────────────────────────
  function loop(now: number) {
    const fps = ANIM_FPS[state] ?? 8;
    if (now - lastFrameTime >= 1000 / fps) {
      lastFrameTime = now;
      tick();
    }
    rafId = requestAnimationFrame(loop);
  }

  function tick() {
    if (state !== prevState) {
      animFrameIndex = 0;
      prevState = state;
    }

    const frames = ANIMS[state] ?? ANIMS.idle;
    if (animFrameIndex >= frames.length) animFrameIndex = 0;

    const img = images[frames[animFrameIndex]];
    ctx.clearRect(0, 0, DISPLAY, DISPLAY);
    if (img?.complete) {
      if (facingLeft) {
        ctx.save();
        ctx.translate(DISPLAY, 0);
        ctx.scale(-1, 1);
      }
      ctx.drawImage(img, 0, 0, DISPLAY, DISPLAY);
      if (facingLeft) ctx.restore();
    }

    animFrameIndex = (animFrameIndex + 1) % frames.length;

    if (!isDragging) updatePhysics();
    maybeUpdateRect();
  }

  function updatePhysics() {
    const gY = window.innerHeight - DISPLAY;

    if (state === 'run') {
      petX += vx;
      const maxX = window.innerWidth - DISPLAY;
      if (petX <= 0)    { petX = 0;    vx =  Math.abs(vx); facingLeft = false; }
      if (petX >= maxX) { petX = maxX; vx = -Math.abs(vx); facingLeft = true;  }

    } else if (state === 'jump' || state === 'fall') {
      vy += FALL_GRAVITY;
      petY += vy;
      if (petY >= gY) {
        petY = gY;
        vy = 0;
        enterState('idle');
        scheduleNextBehavior();
      }

    } else if (state === 'dead' && targetWin) {
      const freshWin = openWindows.find(w => w.title === targetWin!.title);
      if (freshWin) {
        targetWin = freshWin;
        petX = targetWin.x + targetWin.width  / 2 - DISPLAY / 2;
        petY = targetWin.y - DISPLAY + 8;
      }
    }
  }

  function enterState(s: PetState) {
    state = s;
    animFrameIndex = 0;
  }

  // ── IDLE BEHAVIOR ─────────────────────────────────────
  function scheduleNextBehavior() {
    clearTimeout(behaviorTimeout);
    if (state !== 'idle') return;

    if (Math.random() < 0.70) {
      behaviorTimeout = setTimeout(scheduleNextBehavior, 4000 + Math.random() * 5000);
    } else {
      behaviorTimeout = setTimeout(() => {
        if (state !== 'idle') return;
        facingLeft = Math.random() < 0.5;
        vx = facingLeft ? -WALK_SPEED : WALK_SPEED;
        enterState('run');
        behaviorTimeout = setTimeout(() => {
          if (state === 'run') {
            enterState('idle');
            scheduleNextBehavior();
          }
        }, 1500 + Math.random() * 2000);
      }, 2000 + Math.random() * 3000);
    }
  }

  // ── CLICK ─────────────────────────────────────────────
  function handlePetClick() {
    if (state === 'jump' || state === 'fall' || isDragging) return;
    clearTimeout(behaviorTimeout);
    // enterState('hit');
    setTimeout(() => {
      vy = JUMP_FORCE;
      vx = (Math.random() < 0.5 ? 1 : -1) * (2 + Math.random() * 2);
      facingLeft = vx < 0;
      enterState('jump');
    }, 150);
  }
</script>

<div class="overlay">
  <canvas
    bind:this={canvas}
    width={DISPLAY}
    height={DISPLAY}
    class="pet"
    style="left: {petX}px; top: {petY}px; cursor: {isDragging ? 'grabbing' : 'grab'};"
  ></canvas>
</div>

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
    background: transparent;
    pointer-events: none;
  }
  .pet {
    position: absolute;
    image-rendering: pixelated;
    pointer-events: none;
  }
</style>
