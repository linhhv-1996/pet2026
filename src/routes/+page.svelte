<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import * as PIXI from "pixi.js";
  import Matter from "matter-js";
  import { PetSprite } from "../PetSprite";
  import { ChatBubble } from "../ChatBubble";

  // ── Constants ──────────────────────────────────────────
  const DISPLAY = 256;
  const SCALE = DISPLAY / 48;
  const BOTTOM_GAP = 16 * SCALE;
  const WALK_SPEED = 2.2;
  const JUMP_FORCE = -25;
  const MAX_THROW = 18; // px/tick max khi ném

  function getGroundY() {
    return window.innerHeight - DISPLAY + BOTTOM_GAP;
  }

  // ── State machine ─────────────────────────────────────
  type PetState =
    | "Idle"
    | "Idle2"
    | "Idle(Bug)"
    | "Sit"
    | "Sit(Idle)"
    | "Stand"
    | "Shock"
    | "Run"
    | "Jump"
    | "Fall"
    | "Landing"
    | "Glide"
    | "Sleep(Start)"
    | "Sleep"
    | "Awake"
    | "Hurt"
    | "Death"
    | "Dragging"
    | "Attack"
    | "AttackUp";

  const LOCKED_STATES = new Set<PetState>([
    "Death",
    "Hurt",
    "Awake",
    "Landing",
    "Sleep(Start)",
    "Attack",
    "AttackUp",
    "Dragging",
  ]);
  const SLEEP_INTERRUPTIBLE = new Set<PetState>([
    "Idle",
    "Idle2",
    "Idle(Bug)",
    "Sit(Idle)",
    "Run",
    "Shock",
  ]);
  const HURT_INTERRUPTIBLE = new Set<PetState>([
    "Idle",
    "Idle2",
    "Idle(Bug)",
    "Sit",
    "Sit(Idle)",
    "Stand",
    "Shock",
    "Run",
  ]);
  const IDLE_GROUP = new Set<PetState>([
    "Idle",
    "Idle2",
    "Idle(Bug)",
    "Sit(Idle)",
  ]);

  // ── Pet ───────────────────────────────────────────────
  const pet = new PetSprite("/Frog_1.json", "/Frog_1.png", {
    Idle: 7,
    Idle2: 7,
    "Idle(Bug)": 8,
    Run: 10,
    "Sleep(Start)": 6,
    Sleep: 4,
    Awake: 6,
    Jump: 9,
    Fall: 8,
    Landing: 10,
    Hurt: 12,
    Death: 8,
    Sit: 8,
    "Sit(Idle)": 5,
    Stand: 8,
    Shock: 10,
    Glide: 8,
    Attack: 10,
    AttackUp: 10,
  });

  // ── Runtime vars ─────────────────────────────────────
  interface WinInfo {
    title: string;
    x: number;
    y: number;
    width: number;
    height: number;
  }

  let state: PetState = "Idle";
  let petX = 200;
  let petY = 0;

  let dragLastX = 0,
    dragLastY = 0;
  let dragVx = 0,
    dragVy = 0;

  // isThrown: true chỉ khi ném theo hướng NGANG hoặc XUỐNG (không phải thả rơi tự do)
  let isThrown = false;

  // Velocity tick trước — đo impact speed chính xác tại collisionStart
  let prevVx = 0,
    prevVy = 0;

  // Debounce collision
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

  // ── Helpers ───────────────────────────────────────────
  function isIdling() {
    return IDLE_GROUP.has(state);
  }
  function isSleeping() {
    return state === "Sleep(Start)" || state === "Sleep" || state === "Awake";
  }
  function isDragging() {
    return state === "Dragging";
  }
  function isLocked() {
    return LOCKED_STATES.has(state);
  }

  function clearBehavior() {
    clearTimeout(behaviorTimer);
    clearTimeout(attackFallbackTimer);

    if (stunTimer) {
      clearTimeout(stunTimer);
      stunTimer = null;
    }
    // Reset attackCooldown khi hành vi bị clear (drag, death, hurt, v.v.)
    // tránh tình trạng cooldown bị "treo" sau khi state thay đổi đột ngột
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
    bubble?.showMood(
      mood,
      pet.container.x + DISPLAY / 2,
      pet.getHeadY(),
      durationMs,
    );
  }

  function scheduleBubble() {
    clearTimeout(bubbleTimer);
    bubbleTimer = setTimeout(
      () => {
        if (!bubble) {
          scheduleBubble();
          return;
        }
        const secs = (Date.now() - idleStartTime) / 1000;
        if (isSleeping()) showMood("sleep");
        else if (isIdling())
          showMood(
            secs > 30
              ? Math.random() < 0.5
                ? "hungry"
                : "bored"
              : Math.random() < 0.5
                ? "idle"
                : "happy",
          );
        scheduleBubble();
      },
      8000 + Math.random() * 12000,
    );
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

  function enterSleep() {
    clearBehavior();
    playOnce("Sleep(Start)", () => enterState("Sleep"));
  }

  function exitSleep() {
    bubble?.hide();
    playOnce("Awake", () => {
      enterState("Idle");
      scheduleNextBehavior();
    });
  }

  // ── Behavior scheduler ────────────────────────────────
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
            playOnce("Stand", () => {
              enterState("Idle");
              scheduleNextBehavior();
            });
          }, 2000);
        });
      }, 3000);
    } else {
      behaviorTimer = setTimeout(
        () => {
          if (!isIdling()) return;
          pet.facingLeft = Math.random() < 0.5;
          Matter.Body.setVelocity(petBody, {
            x: pet.facingLeft ? -WALK_SPEED : WALK_SPEED,
            y: petBody.velocity.y,
          });
          enterState("Run");
          behaviorTimer = setTimeout(
            () => {
              if (state !== "Run") return;
              Matter.Body.setVelocity(petBody, { x: 0, y: petBody.velocity.y });
              enterState("Idle");
              scheduleNextBehavior();
            },
            1500 + Math.random() * 2000,
          );
        },
        2000 + Math.random() * 3000,
      );
    }
  }

  // ── Physics init ──────────────────────────────────────
  function initPhysics() {
    engine = Matter.Engine.create();
    engine.gravity.y = 1.5;

    petBody = Matter.Bodies.rectangle(
      petX + DISPLAY / 2,
      petY + DISPLAY - BOTTOM_GAP - 35,
      60,
      60,
      { restitution: 0.15, friction: 0.6, frictionAir: 0.05, density: 0.008 },
    );

    const W = window.innerWidth,
      H = window.innerHeight;
    Matter.Composite.add(engine.world, [
      petBody,
      Matter.Bodies.rectangle(W / 2, H + 50, W * 2, 100, {
        isStatic: true,
        friction: 0.6,
        restitution: 0.1,
      }),
      Matter.Bodies.rectangle(-50, H / 2, 100, H * 2, {
        isStatic: true,
        friction: 0.2,
        restitution: 0.05,
      }),
      Matter.Bodies.rectangle(W + 50, H / 2, 100, H * 2, {
        isStatic: true,
        friction: 0.2,
        restitution: 0.05,
      }),
      Matter.Bodies.rectangle(W / 2, -50, W * 2, 100, {
        isStatic: true,
        friction: 0,
        restitution: 0,
      }),
    ]);

    Matter.Events.on(engine, "collisionStart", (event) => {
      for (const pair of event.pairs) {
        if (pair.bodyA !== petBody && pair.bodyB !== petBody) continue;
        if (collisionCooldownTicks > 0) continue;

        const impactSpeed = Math.hypot(prevVx, prevVy);
        // Evaluate shouldKnockout TRƯỚC khi reset isThrown
        // Tránh bug: nếu reset trước, tường phải sẽ không nhận isThrown=true
        // từ lần ném (vì sàn có thể đã reset nó trước đó)
        const shouldKnockout = isThrown || impactSpeed > 16;
        isThrown = false; // reset sau khi đã dùng

        // Cho phép state "Hurt" pass qua để có thể Landing
        // Các state Death/Dragging/Awake/Sleep vẫn bị block hoàn toàn
        if (isLocked() && state !== "Hurt") continue;

        if (shouldKnockout) {
          // Ném mạnh → ngất. Reset vận tốc ngang NGAY để không trượt
          collisionCooldownTicks = COLLISION_COOLDOWN;
          clearBehavior();
          bubble?.hide();
          showMood("scared", 1200);
          // Dừng trượt ngang ngay khi tiếp đất: giữ lại chút vận tốc y để vật lý tắt dần tự nhiên
          Matter.Body.setVelocity(petBody, {
            x: 0,
            y: petBody.velocity.y * 0.3,
          });
          playOnce("Death", () => {
            pet.freeze();
          });
        } else if (impactSpeed > 9) {
          collisionCooldownTicks = COLLISION_COOLDOWN;
          clearBehavior();
          showMood("scared", 800);
          playOnce("Hurt", () => enterState("Fall"));
        } else if (["Fall", "Glide", "Jump", "Shock", "Hurt"].includes(state)) {
          // "Hurt" thêm vào đây: nếu đang Hurt animation mà tiếp đất nhẹ → Landing bình thường
          collisionCooldownTicks = COLLISION_COOLDOWN;
          playOnce("Landing", () => {
            enterState("Idle");
            scheduleNextBehavior();
          });
        }

        break;
      }
    });
  }

  // ── Game tick ─────────────────────────────────────────
  function gameTick(ticker: PIXI.Ticker) {
    if (!isDragging()) {
      prevVx = petBody.velocity.x;
      prevVy = petBody.velocity.y;

      Matter.Engine.update(engine, ticker.deltaMS);

      if (collisionCooldownTicks > 0) collisionCooldownTicks--;

      if (state === "Run") {
        const cx = petBody.position.x;
        if (cx < 60 && pet.facingLeft) pet.facingLeft = false;
        else if (cx > window.innerWidth - 60 && !pet.facingLeft)
          pet.facingLeft = true;
        Matter.Body.setVelocity(petBody, {
          x: pet.facingLeft ? -WALK_SPEED : WALK_SPEED,
          y: petBody.velocity.y,
        });
      }

      // Wake-up sau Death
      if (state === "Death") {
        if (petBody.speed < 0.3) {
          if (!stunTimer) {
            stunTimer = setTimeout(() => {
              stunTimer = null;
              // Reset attackCooldown tường minh ở đây — đảm bảo attack
              // hoạt động ngay sau khi dậy, bất kể lịch sử trước đó
              attackCooldown = false;
              playOnce("Awake", () => {
                enterState("Idle");
                scheduleNextBehavior();
              });
            }, 2000);
          }
        } else if (stunTimer) {
          clearTimeout(stunTimer);
          stunTimer = null;
        }
      }

      petX = petBody.position.x - DISPLAY / 2;
      petY = petBody.position.y + 35 - DISPLAY + BOTTOM_GAP;

      if (!isLocked() && Math.abs(petBody.velocity.x) > 0.5)
        pet.facingLeft = petBody.velocity.x < 0;

      if (petBody.velocity.y > 4 && !isLocked() && state !== "Fall")
        enterState("Fall");
    }

    pet.setPosition(petX, petY);
    bubble?.updatePosition(pet.container.x + DISPLAY / 2, pet.getHeadY());
    maybeUpdateRect();
  }

  // ── Cursor Attack ─────────────────────────────────────
  function checkCursorAttack(mouseX: number, mouseY: number) {
    if (isDragging() || attackCooldown) return;
    const canAttack = isIdling() || ["Shock", "Fall", "Run"].includes(state);
    if (!canAttack) return;

    const cx = petX + DISPLAY / 2;
    const cy = petY + DISPLAY / 2;
    
    // Dùng tọa độ từ hệ điều hành thay vì e.clientX / e.clientY
    const dx = mouseX - cx; 
    const dy = mouseY - cy;
    const dist = Math.hypot(dx, dy);

    if (dist >= 70) return;

    clearBehavior();
    attackCooldown = true;
    pet.facingLeft = dx < 0;
    showMood("angry", 1000);
    const isAbove = dy < 0 && Math.abs(dy) > Math.abs(dx) * 0.7;
    const animName = isAbove ? "AttackUp" : "Attack";

    // Chạy animation
    playOnce(animName, () => {
      clearTimeout(attackFallbackTimer); // Thành công thì hủy fallback ngay
      attackCooldown = false;
      enterState("Idle");
      scheduleNextBehavior();
    });

    // Fallback an toàn: Reset cả Cooldown LẪN State
    attackFallbackTimer = setTimeout(() => {
      attackCooldown = false;
      // Chỉ can thiệp nếu state thực sự đang bị kẹt ở Attack
      if (state === "Attack" || state === "AttackUp") {
        enterState("Idle");
        scheduleNextBehavior();
      }
    }, 1500);
  }

  // ── Click handling ────────────────────────────────────
  function handlePetClick() {
    if (isDragging()) return;
    if (isSleeping()) {
      exitSleep();
      return;
    }
    if (
      [
        "Jump",
        "Fall",
        "Landing",
        "Glide",
        "Death",
        "Attack",
        "AttackUp",
      ].includes(state)
    )
      return;

    clickCount++;
    clearTimeout(clickTimer);
    clickTimer = setTimeout(() => {
      clickCount = 0;
    }, 800);

    if (clickCount >= 5) {
      clickCount = 0;
      clearBehavior();
      bubble?.hide();
      playOnce("Death", () => {
        pet.freeze();
      });
      Matter.Body.applyForce(petBody, petBody.position, {
        x: (Math.random() - 0.5) * 0.04,
        y: -0.02,
      });
      return;
    }

    if (!HURT_INTERRUPTIBLE.has(state)) return;
    showMood("scared", 1500);
    clearBehavior();
    playOnce("Hurt", () => {
      const jumpX = (Math.random() < 0.5 ? 1 : -1) * (4 + Math.random() * 4);
      Matter.Body.setVelocity(petBody, { x: jumpX, y: JUMP_FORCE });
      pet.facingLeft = jumpX < 0;
      enterState("Jump");
    });
  }

  // ── Mount / Destroy ───────────────────────────────────
  onMount(async () => {
    petY = getGroundY() - 200;
    initPhysics();

    pixiApp = new PIXI.Application();
    await pixiApp.init({
      width: window.innerWidth,
      height: window.innerHeight,
      backgroundAlpha: 0,
      antialias: false,
      autoDensity: true,
      resolution: window.devicePixelRatio || 1,
    });
    PIXI.TextureSource.defaultOptions.scaleMode = "nearest";

    canvasEl = pixiApp.canvas as HTMLCanvasElement;
    Object.assign(canvasEl.style, {
      position: "fixed",
      top: "0",
      left: "0",
      width: "100vw",
      height: "100vh",
      imageRendering: "pixelated",
      pointerEvents: "none",
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

    // ── Rust callbacks ───────────────────────────────────
    (window as any).__onMouseMove = (x: number, y: number) => checkCursorAttack(x, y);

    (window as any).__onPetClicked = () => handlePetClick();

    (window as any).__onPetDragStart = () => {
      clearBehavior();
      if (stunTimer) {
        clearTimeout(stunTimer);
        stunTimer = null;
      }
      state = "Dragging";
      pet.playOnce("Shock", () => {
        if (isDragging()) pet.play("Shock");
      });
      showMood("scared", 1200);
      dragLastX = petX;
      dragLastY = petY;
      dragVx = 0;
      dragVy = 0;
    };

    (window as any).__onPetDrag = (x: number, y: number) => {
      Matter.Body.setPosition(petBody, {
        x: x + DISPLAY / 2,
        y: y + DISPLAY - BOTTOM_GAP - 35,
      });
      Matter.Body.setVelocity(petBody, { x: 0, y: 0 });
      dragVx = x - dragLastX;
      dragVy = y - dragLastY;
      dragLastX = x;
      dragLastY = y;
      petX = x;
      petY = y;
    };

    (window as any).__onPetDragEnd = () => {
      bubble?.hide();
      collisionCooldownTicks = 0;

      const speed = Math.hypot(dragVx, dragVy);

      // isThrown = true CHỈ KHI ném theo hướng ngang hoặc xuống
      // Kéo thẳng lên rồi thả (dragVy << 0 và |dragVx| nhỏ) → không phải ném, chỉ thả rơi
      const isThrownHorizontalOrDown = speed > 6 && dragVy > -3;

      if (isThrownHorizontalOrDown) {
        isThrown = true;
        const factor = Math.min(1.2, MAX_THROW / speed);
        Matter.Body.setVelocity(petBody, {
          x: dragVx * factor,
          y: dragVy * factor,
        });
      } else {
        // Thả rơi tự do (kéo lên thả): để vật lý tự handle, chỉ add vận tốc ngang nhỏ nếu có
        isThrown = false;
        Matter.Body.setVelocity(petBody, {
          x: dragVx * 0.3, // giữ chút momentum ngang nếu có
          y: 0, // reset vận tốc y — để gravity kéo xuống tự nhiên
        });
      }

      // Sau drag, Shock ngắn rồi chuyển Fall
      playOnce("Shock", () => enterState("Fall"));
      invoke("update_pet_rect", { x: petX, y: petY, w: DISPLAY, h: DISPLAY });
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
    Matter.Engine.clear(engine);
    if (engine?.world) Matter.Composite.clear(engine.world, false);
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
    delete (window as any).__onMouseMove;
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
