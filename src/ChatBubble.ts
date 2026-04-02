import * as PIXI from 'pixi.js';

// ── Spritesheet config ─────────────────────────────────────────
const SHEET_URL = '/bubble.png';
const CELL      = 16;   // px per cell in source sheet

/**
 * Mỗi "set" bubble gồm 3 frame liên tiếp (col, col+1, col+2) trên cùng 1 row.
 * Đặt tên tạm theo row/col — mày đổi key sau tùy ý.
 *
 * row  = row index (0-based) trong sheet
 * col  = col index của frame đầu tiên
 */
export const BUBBLE_SETS = {
  // ── Row 1 (idx 0) ───────────────────────────────────────────
  bubble_r1_s1:  { row: 0,  col: 0  },
  bubble_r1_s2:  { row: 0,  col: 3  },
  bubble_r1_s3:  { row: 0,  col: 6  },
  bubble_r1_s4:  { row: 0,  col: 9  },
  // ── Row 3 (idx 2) ───────────────────────────────────────────
  bubble_r3_s1:  { row: 2,  col: 0  },
  bubble_r3_s2:  { row: 2,  col: 3  },
  bubble_r3_s3:  { row: 2,  col: 6  },
  bubble_r3_s4:  { row: 2,  col: 9  },
  // ── Row 5 (idx 4) ───────────────────────────────────────────
  bubble_r5_s1:  { row: 4,  col: 0  },
  bubble_r5_s2:  { row: 4,  col: 3  },
  bubble_r5_s3:  { row: 4,  col: 6  },
  bubble_r5_s4:  { row: 4,  col: 9  },
  // ── Row 7 (idx 6) ───────────────────────────────────────────
  bubble_r7_s1:  { row: 6,  col: 0  },
  bubble_r7_s2:  { row: 6,  col: 3  },
  bubble_r7_s3:  { row: 6,  col: 6  },
  bubble_r7_s4:  { row: 6,  col: 9  },
  // ── Row 9 (idx 8) ───────────────────────────────────────────
  bubble_r9_s1:  { row: 8,  col: 0  },
  bubble_r9_s2:  { row: 8,  col: 3  },
  bubble_r9_s3:  { row: 8,  col: 6  },
  bubble_r9_s4:  { row: 8,  col: 9  },
  // ── Row 11 (idx 10) ─────────────────────────────────────────
  bubble_r11_s1: { row: 10, col: 0  },
  bubble_r11_s2: { row: 10, col: 3  },
  bubble_r11_s3: { row: 10, col: 6  },
  bubble_r11_s4: { row: 10, col: 9  },
  // ── Row 13 (idx 12) — misc, xử lý riêng ────────────────────
  bubble_r13_misc: { row: 12, col: 0 },
} as const;

export type BubbleSetName = keyof typeof BUBBLE_SETS;

/**
 * Map mood → tên set bubble.
 * Đổi value thành bất kỳ key nào trong BUBBLE_SETS tùy ý.
 */
export const MOOD_MAP: Record<string, BubbleSetName> = {
  idle:    'bubble_r1_s1',
  happy:   'bubble_r1_s2',
  bored:   'bubble_r3_s1',
  hungry:  'bubble_r3_s2',
  scared:  'bubble_r5_s1',
  sleep:   'bubble_r7_s1',
};

// ── ChatBubble class ───────────────────────────────────────────

export class ChatBubble {
  private frames:   Record<string, PIXI.Texture[]> = {};  // setName → [f0,f1,f2]

  private container = new PIXI.Container();
  private sprite:   PIXI.AnimatedSprite | null = null;

  private hideTimer: ReturnType<typeof setTimeout> | null = null;
  private _visible  = false;

  // Scale để bubble to hơn — 16px cell → match pet size (256px / 16cell ≈ 5x)
  private readonly RENDER_SCALE = 3;  // 16*3 = 48px — vừa đủ, không quá to
  // Tốc độ animation (frame/s)
  private readonly FPS = 8;

  constructor(private stage: PIXI.Container) {
    this.container.visible = false;
    stage.addChild(this.container);
  }

  async load() {
    const baseTex = await PIXI.Assets.load(SHEET_URL);

    // Pre-slice tất cả sets
    for (const [name, def] of Object.entries(BUBBLE_SETS)) {
      const { row, col } = def;
      // Mỗi set = 3 frame, trừ r13_misc thì 1 frame thôi
      const frameCount = name === 'bubble_r13_misc' ? 1 : 3;
      const textures: PIXI.Texture[] = [];
      for (let i = 0; i < frameCount; i++) {
        const frame = new PIXI.Rectangle(
          (col + i) * CELL,
          row * CELL,
          CELL,
          CELL,
        );
        textures.push(new PIXI.Texture({ source: baseTex.source, frame }));
      }
      this.frames[name] = textures;
    }

    console.log('[ChatBubble] loaded', Object.keys(this.frames).length, 'sets');
  }

  /** Hiện bubble theo tên set.
   *  centerX  = tâm ngang của pet (stage coords)
   *  headY    = top pixel thật của ếch (từ pet.getHeadY())
   */
  showSet(setName: BubbleSetName, centerX: number, headY: number, durationMs = 2800) {
    const textures = this.frames[setName];
    if (!textures?.length) {
      console.warn('[ChatBubble] unknown set:', setName);
      return;
    }
    this._buildSprite(textures, centerX, headY);
    this._scheduleHide(durationMs);
  }

  /** Hiện bubble theo mood (dùng MOOD_MAP) */
  showMood(mood: keyof typeof MOOD_MAP, centerX: number, headY: number, durationMs = 2800) {
    const setName = MOOD_MAP[mood];
    if (!setName) return;
    this.showSet(setName, centerX, headY, durationMs);
  }

  /** Random 1 trong các set bubble */
  showRandom(centerX: number, headY: number, durationMs = 2800) {
    const keys = Object.keys(this.frames) as BubbleSetName[];
    const pick  = keys[Math.floor(Math.random() * keys.length)];
    this.showSet(pick, centerX, headY, durationMs);
  }

  private _buildSprite(textures: PIXI.Texture[], centerX: number, headY: number) {
    if (this.sprite) {
      this.sprite.stop();
      this.container.removeChild(this.sprite);
      this.sprite.destroy();
      this.sprite = null;
    }

    const s = new PIXI.AnimatedSprite(textures);
    s.animationSpeed = this.FPS / 60;
    s.loop  = true;
    s.scale.set(this.RENDER_SCALE);
    s.play();

    this.container.addChild(s);
    this.sprite   = s;
    this._visible = true;
    this.container.visible = true;

    this._setPosition(centerX, headY);
  }

  private _setPosition(centerX: number, headY: number) {
    const rendered = CELL * this.RENDER_SCALE;
    // bubble bottom sát đầu ếch, gap 4px
    this.container.x = centerX - rendered / 2;
    this.container.y = headY - rendered - 4;
  }

  /** Gọi mỗi frame từ gameTick để bubble theo pet */
  updatePosition(centerX: number, headY: number) {
    if (!this._visible) return;
    this._setPosition(centerX, headY);
  }

  hide() {
    if (this.hideTimer) { clearTimeout(this.hideTimer); this.hideTimer = null; }
    this.sprite?.stop();
    this.container.visible = false;
    this._visible = false;
  }

  private _scheduleHide(ms: number) {
    if (this.hideTimer) clearTimeout(this.hideTimer);
    this.hideTimer = setTimeout(() => this.hide(), ms);
  }

  get isVisible() { return this._visible; }

  destroy() {
    this.hide();
    this.container.destroy({ children: true });
    for (const texList of Object.values(this.frames)) {
      texList.forEach(t => t.destroy());
    }
  }
}
