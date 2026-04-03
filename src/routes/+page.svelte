<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import * as PIXI from "pixi.js";
  import Matter from "matter-js";
  import { PetSprite } from "../PetSprite";
  import { ChatBubble } from "../ChatBubble";

  const DISPLAY = 256;
  const SCALE = DISPLAY / 48;
  const BOTTOM_GAP = 16 * SCALE;
  const WALK_SPEED = 2.2;
  const JUMP_FORCE = -25;
  const MAX_THROW = 18;

  function getGroundY() {
    return window.innerHeight - DISPLAY + BOTTOM_GAP;
  }

  type PetState =
    | "Idle" | "Idle2" | "Idle(Bug)" | "Sit" | "Sit(Idle)" | "Stand"
    | "Shock" | "Run" | "Jump" | "Fall" | "Landing" | "Glide"
    | "Sleep(Start)" | "Sleep" | "Awake" | "Hurt" | "Death"
    | "Dragging" | "Attack" | "AttackUp";

  const LOCKED_STATES = new Set<PetState>(["Death","Hurt","Awake","Landing","Sleep(Start)","Attack","AttackUp","Dragging"]);
  const SLEEP_INTERRUPTIBLE = new Set<PetState>(["Idle","Idle2","Idle(Bug)","Sit(Idle)","Run","Shock"]);
  const HURT_INTERRUPTIBLE  = new Set<PetState>(["Idle","Idle2","Idle(Bug)","Sit","Sit(Idle)","Stand","Shock","Run"]);
  const IDLE_GROUP          = new Set<PetState>(["Idle","Idle2","Idle(Bug)","Sit(Idle)"]);
  
  const FROGS = ["Frog_1", "Frog_2", "Frog_3", "Frog_4"] as const;
  type FrogId = (typeof FROGS)[number];

  const FRAME_COUNTS = {
    Idle: 7, Idle2: 7, "Idle(Bug)": 8, Run: 10,
    "Sleep(Start)": 6, Sleep: 4, Awake: 6,
    Jump: 9, Fall: 8, Landing: 10,
    Hurt: 12, Death: 8, Sit: 8, "Sit(Idle)": 5,
    Stand: 8, Shock: 10, Glide: 8, Attack: 10, AttackUp: 10,
  };
  let pet = new PetSprite("/Frog_1.json", "/Frog_1.png", FRAME_COUNTS);

  async function reloadSprite(frog: FrogId) {
    if (!pixiApp) return;
    pixiApp.ticker.remove(gameTick);
    clearBehavior();
    if (stunTimer) { clearTimeout(stunTimer); stunTimer = null; }

    const wasFacingLeft = pet.facingLeft;
    const savedX = petX, savedY = petY;

    pet.destroy();
    
    pet = new PetSprite(`/${frog}.json`, `/${frog}.png`, FRAME_COUNTS);
    await pet.load(pixiApp, DISPLAY);

    pet.setPosition(savedX, savedY);
    pet.facingLeft = wasFacingLeft;
    
    state = "Idle";
    pixiApp.ticker.add(gameTick);
    enterState("Idle");
    scheduleNextBehavior();
  }

  interface WinInfo { title: string; x: number; y: number; width: number; height: number; }

  let state: PetState = "Idle";
  let petX = 200, petY = 0;
  let dragLastX = 0, dragLastY = 0;
  let dragVx = 0, dragVy = 0;
  let isThrown = false;
  let prevVx = 0, prevVy = 0;
  let collisionCooldownTicks = 0;
  const COLLISION_COOLDOWN = 8;
  let attackCooldown = false;
  let clickCount = 0;
  let clickTimer: ReturnType<typeof setTimeout>;
  let behaviorTimer: ReturnType<typeof setTimeout>;
  let attackFallbackTimer: ReturnType<typeof setTimeout>;
  let bubbleTimer: ReturnType<typeof setTimeout>;
  let stunTimer: ReturnType<typeof setTimeout> | null = null;
  let bubble: ChatBubble;
  let idleStartTime = Date.now();
  let openWindows: WinInfo[] = [];
  let pixiApp: PIXI.Application;
  let canvasEl: HTMLCanvasElement;
  let lastRectUpdate = 0;
  let engine: Matter.Engine;
  let petBody: Matter.Body;
  let isPetHidden = false;

  function isIdling()   { return IDLE_GROUP.has(state); }
  function isSleeping() { return state === "Sleep(Start)" || state === "Sleep" || state === "Awake"; }
  function isDragging() { return state === "Dragging"; }
  function isLocked()   { return LOCKED_STATES.has(state); }

  function clearBehavior() {
    clearTimeout(behaviorTimer);
    clearTimeout(attackFallbackTimer);
    if (stunTimer) { clearTimeout(stunTimer); stunTimer = null; }
    attackCooldown = false;
  }

  function maybeUpdateRect() {
    if (isDragging()) return;
    const now = performance.now();
    if (now - lastRectUpdate > 32) {
      lastRectUpdate = now;
      invoke("update_pet_rect", { x: petX, y: petY, w: DISPLAY, h: DISPLAY });
    }
  }

  function showMood(mood: string, durationMs = 2800) {
    bubble?.showMood(mood, pet.container.x + DISPLAY / 2, pet.getHeadY(), durationMs);
  }

  function scheduleBubble() {
    clearTimeout(bubbleTimer);
    bubbleTimer = setTimeout(() => {
      if (!bubble) { scheduleBubble(); return; }
      const secs = (Date.now() - idleStartTime) / 1000;
      if (isSleeping()) showMood("sleep");
      else if (isIdling())
        showMood(secs > 30 ? (Math.random() < 0.5 ? "hungry" : "bored") : (Math.random() < 0.5 ? "idle" : "happy"));
      scheduleBubble();
    }, 8000 + Math.random() * 12000);
  }

  function enterState(s: PetState) {
    if (IDLE_GROUP.has(s) && !IDLE_GROUP.has(state)) idleStartTime = Date.now();
    state = s;
    pet.play(s);
  }

  function playOnce(s: PetState, onDone: () => void) {
    state = s;
    pet.playOnce(s, onDone);
  }

  function enterSleep() { clearBehavior(); playOnce("Sleep(Start)", () => enterState("Sleep")); }
  function exitSleep()  { bubble?.hide(); playOnce("Awake", () => { enterState("Idle"); scheduleNextBehavior(); }); }

  function scheduleNextBehavior() {
    clearBehavior();
    if (!isIdling()) return;
    if (Math.random() < 0.3) {
      behaviorTimer = setTimeout(() => {
        if (!isIdling()) return;
        playOnce("Sit", () => {
          enterState("Sit(Idle)");
          behaviorTimer = setTimeout(() => {
            if (!isIdling()) return;
            playOnce("Stand", () => { enterState("Idle"); scheduleNextBehavior(); });
          }, 2000);
        });
      }, 3000);
    } else {
      behaviorTimer = setTimeout(() => {
        if (!isIdling()) return;
        pet.facingLeft = Math.random() < 0.5;
        Matter.Body.setVelocity(petBody, { x: pet.facingLeft ? -WALK_SPEED : WALK_SPEED, y: petBody.velocity.y });
        enterState("Run");
        behaviorTimer = setTimeout(() => {
          if (state !== "Run") return;
          Matter.Body.setVelocity(petBody, { x: 0, y: petBody.velocity.y });
          enterState("Idle");
          scheduleNextBehavior();
        }, 1500 + Math.random() * 2000);
      }, 2000 + Math.random() * 3000);
    }
  }

  function initPhysics() {
    engine = Matter.Engine.create();
    engine.gravity.y = 1.5;
    petBody = Matter.Bodies.rectangle(
      petX + DISPLAY / 2, petY + DISPLAY - BOTTOM_GAP - 35, 60, 60,
      { restitution: 0.15, friction: 0.6, frictionAir: 0.05, density: 0.008 },
    );
    const W = window.innerWidth, H = window.innerHeight;
    Matter.Composite.add(engine.world, [
      petBody,
      Matter.Bodies.rectangle(W / 2, H + 50, W * 2, 100, { isStatic: true, friction: 0.6 }),
      Matter.Bodies.rectangle(-50, H / 2, 100, H * 2, { isStatic: true }),
      Matter.Bodies.rectangle(W + 50, H / 2, 100, H * 2, { isStatic: true }),
    ]);
    const events = Matter.Events;
    events.on(engine, "collisionStart", ({ pairs }) => {
      for (const p of pairs) {
        const isMyCollision = p.bodyA === petBody || p.bodyB === petBody;
        if (!isMyCollision) continue;
        if (collisionCooldownTicks > 0) continue;

        const speed = Math.hypot(prevVx, prevVy);
        const isHardLanding = prevVy > 14 && Math.abs(prevVx) < Math.abs(prevVy) * 1.5;
        const isSideHit = isThrown && speed > 8 && !isHardLanding;

        if (isHardLanding || isSideHit) {
          isThrown = false;
          collisionCooldownTicks = COLLISION_COOLDOWN;
          clearBehavior();
          const vol = Math.min(1, speed / 20);
          if (speed > 18) {
            playOnce("Death", () => { pet.freeze(); });
            Matter.Body.applyForce(petBody, petBody.position, {
              x: (Math.random() - 0.5) * 0.02, y: -0.01 * vol,
            });
          } else {
            playOnce("Hurt", () => {
              const jumpX = (Math.random() < 0.5 ? 1 : -1) * (2 + Math.random() * 3);
              Matter.Body.setVelocity(petBody, { x: jumpX, y: -8 });
              pet.facingLeft = jumpX < 0;
              enterState("Jump");
            });
          }
        }
      }
    });
  }

  function gameTick(ticker: PIXI.Ticker) {
    if (isPetHidden) return;

    if (!isDragging()) {
      prevVx = petBody.velocity.x;
      prevVy = petBody.velocity.y;
      
      // FIX LỖI NỔ VẬT LÝ KHI LAG/MỚI BẬT: Giới hạn tối đa bước nhảy thời gian là 32ms
      const safeDelta = Math.min(ticker.deltaMS, 32);
      Matter.Engine.update(engine, safeDelta);
      
      if (collisionCooldownTicks > 0) collisionCooldownTicks--;
      
      if (state === "Run") {
        const cx = petBody.position.x;
        if (cx < 60 && pet.facingLeft) pet.facingLeft = false;
        else if (cx > window.innerWidth - 60 && !pet.facingLeft) pet.facingLeft = true;
        Matter.Body.setVelocity(petBody, {
          x: pet.facingLeft ? -WALK_SPEED : WALK_SPEED, y: petBody.velocity.y,
        });
      }

      if (state === "Death") {
        if (petBody.speed < 0.3) {
          if (!stunTimer) {
            stunTimer = setTimeout(() => {
              stunTimer = null; attackCooldown = false;
              playOnce("Awake", () => { enterState("Idle"); scheduleNextBehavior(); });
            }, 2000);
          }
        } else if (stunTimer) { clearTimeout(stunTimer); stunTimer = null; }
      }

      petX = petBody.position.x - DISPLAY / 2;
      petY = petBody.position.y + 35 - DISPLAY + BOTTOM_GAP;
      if (!isLocked() && Math.abs(petBody.velocity.x) > 0.5) pet.facingLeft = petBody.velocity.x < 0;
      
      if (petBody.velocity.y > 4 && !isLocked() && state !== "Fall") enterState("Fall");
      if (state === "Fall" && Math.abs(petBody.velocity.y) < 1.5) {
        collisionCooldownTicks = COLLISION_COOLDOWN;
        playOnce("Landing", () => { enterState("Idle"); scheduleNextBehavior(); });
      }
    }

    pet.setPosition(petX, petY);
    bubble?.updatePosition(pet.container.x + DISPLAY / 2, pet.getHeadY());
    maybeUpdateRect();
  }

  function checkCursorAttack(mouseX: number, mouseY: number) {
    if (isDragging() || attackCooldown) return;
    const canAttack = isIdling() || ["Shock","Fall","Run"].includes(state);
    if (!canAttack) return;
    const cx = petX + DISPLAY / 2, cy = petY + DISPLAY / 2;
    const dx = mouseX - cx, dy = mouseY - cy;
    if (Math.hypot(dx, dy) >= 70) return;
    clearBehavior();
    attackCooldown = true; pet.facingLeft = dx < 0; showMood("angry", 1000);
    const isAbove = dy < 0 && Math.abs(dy) > Math.abs(dx) * 0.7;
    playOnce(isAbove ? "AttackUp" : "Attack", () => {
      clearTimeout(attackFallbackTimer); attackCooldown = false; enterState("Idle"); scheduleNextBehavior();
    });
    attackFallbackTimer = setTimeout(() => {
      attackCooldown = false;
      if (state === "Attack" || state === "AttackUp") { enterState("Idle"); scheduleNextBehavior(); }
    }, 1500);
  }

  function handlePetClick() {
    if (isDragging()) return;
    if (isSleeping()) { exitSleep(); return; }
    if (["Jump","Fall","Landing","Glide","Death","Attack","AttackUp"].includes(state)) return;
    clickCount++;
    clearTimeout(clickTimer);
    clickTimer = setTimeout(() => { clickCount = 0; }, 800);
    if (clickCount >= 5) {
      clickCount = 0; clearBehavior(); bubble?.hide();
      playOnce("Death", () => { pet.freeze(); });
      Matter.Body.applyForce(petBody, petBody.position, { x: (Math.random() - 0.5) * 0.04, y: -0.02 });
      return;
    }
    if (!HURT_INTERRUPTIBLE.has(state)) return;
    showMood("scared", 1500); clearBehavior();
    playOnce("Hurt", () => {
      const jumpX = (Math.random() < 0.5 ? 1 : -1) * (4 + Math.random() * 4);
      Matter.Body.setVelocity(petBody, { x: jumpX, y: JUMP_FORCE });
      pet.facingLeft = jumpX < 0; enterState("Jump");
    });
  }

  onMount(async () => {
    petY = getGroundY() - 200;
    initPhysics();

    pixiApp = new PIXI.Application();
    await pixiApp.init({
      width: window.innerWidth, height: window.innerHeight,
      backgroundAlpha: 0, antialias: false,
      autoDensity: true, resolution: window.devicePixelRatio || 1,
    });
    PIXI.TextureSource.defaultOptions.scaleMode = "nearest";

    canvasEl = pixiApp.canvas as HTMLCanvasElement;
    Object.assign(canvasEl.style, {
      position: "fixed", top: "0", left: "0", width: "100vw", height: "100vh",
      imageRendering: "pixelated", pointerEvents: "none",
    });
    document.body.appendChild(canvasEl);

    await pet.load(pixiApp, DISPLAY);
    bubble = new ChatBubble(pixiApp.stage);
    await bubble.load();

    invoke("update_pet_rect", { x: petX, y: petY, w: DISPLAY, h: DISPLAY });
    await invoke("request_accessibility");

    pixiApp.ticker.add(gameTick);
    enterState("Fall");
    scheduleNextBehavior();
    scheduleBubble();

    (window as any).__onFrogChanged = (frog: string) => { reloadSprite(frog as FrogId); };
    (window as any).__onMouseMove    = (x: number, y: number) => checkCursorAttack(x, y);
    (window as any).__onPetClicked   = () => handlePetClick();

    (window as any).__onPetDragStart = () => {
      clearBehavior(); if (stunTimer) { clearTimeout(stunTimer); stunTimer = null; }
      state = "Dragging"; pet.playOnce("Shock", () => { if (isDragging()) pet.play("Shock"); });
      showMood("scared", 1200); dragLastX = petX; dragLastY = petY; dragVx = 0; dragVy = 0;
    };

    (window as any).__onPetDrag = (x: number, y: number) => {
      Matter.Body.setPosition(petBody, { x: x + DISPLAY / 2, y: y + DISPLAY - BOTTOM_GAP - 35 });
      Matter.Body.setVelocity(petBody, { x: 0, y: 0 });
      dragVx = x - dragLastX; dragVy = y - dragLastY; dragLastX = x; dragLastY = y; petX = x; petY = y;
    };
    
    (window as any).__onPetDragEnd = () => {
      bubble?.hide(); collisionCooldownTicks = 0;
      const speed = Math.hypot(dragVx, dragVy);
      const isThrownHorizontalOrDown = speed > 6 && dragVy > -3;
      
      if (isThrownHorizontalOrDown) {
        isThrown = true; const factor = Math.min(1.2, MAX_THROW / speed);
        Matter.Body.setVelocity(petBody, { x: dragVx * factor, y: dragVy * factor });
      } else {
        isThrown = false; Matter.Body.setVelocity(petBody, { x: dragVx * 0.3, y: 0 });
      }
      playOnce("Shock", () => enterState("Fall"));
      invoke("update_pet_rect", { x: petX, y: petY, w: DISPLAY, h: DISPLAY });
    };
    
    (window as any).__onUserAFK    = () => { if (isDragging() || isSleeping()) return; if (!SLEEP_INTERRUPTIBLE.has(state)) return; enterSleep(); };
    (window as any).__onUserActive = () => { if (!isSleeping()) return; exitSleep(); };
    (window as any).__onWindowsUpdated = (wins: WinInfo[]) => { openWindows = wins; };
    
    (window as any).__onPetHide = () => {
      isPetHidden = true; clearBehavior(); clearTimeout(bubbleTimer); bubble?.hide();
      pet.container.visible = false; invoke("update_pet_rect", { x: -9999, y: -9999, w: 0, h: 0 });
    };
    
    (window as any).__onPetShow = () => {
      isPetHidden = false; pet.container.visible = true; petX = 200; petY = getGroundY() - 200;
      Matter.Body.setPosition(petBody, { x: petX + DISPLAY / 2, y: petY + DISPLAY - BOTTOM_GAP - 35 });
      Matter.Body.setVelocity(petBody, { x: 0, y: 0 });
      invoke("update_pet_rect", { x: petX, y: petY, w: DISPLAY, h: DISPLAY });
      enterState("Fall"); scheduleNextBehavior(); scheduleBubble();
    };
  });

  onDestroy(() => {
    Matter.Engine.clear(engine); if (engine?.world) Matter.Composite.clear(engine.world, false);
    clearBehavior(); clearTimeout(clickTimer); clearTimeout(bubbleTimer);
    bubble?.destroy(); pet.destroy(); canvasEl?.remove(); pixiApp?.destroy(true);
    const w = window as any;
    delete w.__onFrogChanged; delete w.__onPetClicked; delete w.__onPetDragStart; 
    delete w.__onPetDrag; delete w.__onPetDragEnd; delete w.__onUserAFK; 
    delete w.__onUserActive; delete w.__onWindowsUpdated; delete w.__onMouseMove;
    delete w.__onPetHide; delete w.__onPetShow;
  });
</script>

<div class="overlay"></div>

<style>
  :global(body) { margin: 0; background: transparent; overflow: hidden; user-select: none; }
  .overlay { position: fixed; inset: 0; pointer-events: none; }
</style>
