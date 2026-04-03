import * as PIXI from 'pixi.js';

export class PetSprite {
  private jsonUrl: string;
  private imgUrl:  string;
  private fpsMap:  Record<string, number>;

  private sheet:         PIXI.Spritesheet | null = null;
  private tags:          { name: string; from: number; to: number }[] = [];
  private allFrameKeys:  string[] = [];

  private sprite:  PIXI.AnimatedSprite | null = null;
  private _size    = 0;
  private _scale   = 1;
  private _cur     = '';
  private _left    = false;

  /**
   * Generation counter — tăng mỗi lần build() được gọi.
   * Callback của playOnce chỉ chạy nếu generation không thay đổi kể từ lúc nó được tạo.
   * Cách này loại bỏ hoàn toàn race condition stale callback mà không cần flag phức tạp.
   */
  private _gen = 0;

  public container = new PIXI.Container();

  constructor(jsonUrl: string, imgUrl: string, fpsMap: Record<string, number> = {}) {
    this.jsonUrl = jsonUrl;
    this.imgUrl  = imgUrl;
    this.fpsMap  = fpsMap;
  }

  async load(app: PIXI.Application, renderSize: number) {
    this._size  = renderSize;
    this._scale = renderSize / 48;

    const res      = await fetch(this.jsonUrl);
    const jsonData = await res.json();
    const baseTex  = await PIXI.Assets.load(this.imgUrl);

    this.tags         = jsonData.meta.frameTags;
    this.allFrameKeys = Object.keys(jsonData.frames);

    this.sheet = new PIXI.Spritesheet(baseTex, jsonData);
    await this.sheet.parse();

    app.stage.addChild(this.container);
  }

  private getTextures(animName: string): PIXI.Texture[] {
    const tag = this.tags.find(t => t.name.toLowerCase() === animName.toLowerCase());
    if (!tag) {
      console.warn(`[PetSprite] unknown anim: "${animName}". Have: ${this.tags.map(t => t.name).join(', ')}`);
      return [];
    }
    const out: PIXI.Texture[] = [];
    for (let i = tag.from; i <= tag.to; i++) {
      const tex = this.sheet!.textures[this.allFrameKeys[i]];
      if (tex) out.push(tex);
    }
    return out;
  }

  private getFPS(animName: string): number {
    if (this.fpsMap[animName]) return this.fpsMap[animName];
    const tag = this.tags.find(t => t.name.toLowerCase() === animName.toLowerCase());
    if (!tag) return 8;
    const dur = (this.sheet as any).data?.frames?.[this.allFrameKeys[tag.from]]?.duration ?? 100;
    return Math.round(1000 / dur);
  }

  /** Xây sprite mới, gán callback chống stale bằng gen ID. */
  private build(name: string, onComplete?: () => void) {
    const textures = this.getTextures(name);
      if (!textures.length) {
      // SỬA Ở ĐÂY: Nếu không có ảnh, gọi luôn callback để thả tự do cho state
      console.warn(`[PetSprite] Bỏ qua play() vì thiếu texture: ${name}`);
      if (onComplete) onComplete();
      return;
    }

    // Tăng gen — mọi callback cũ của gen trước sẽ bị vô hiệu hoá
    const myGen = ++this._gen;

    if (this.sprite) {
      this.sprite.stop();
      this.sprite.onComplete = undefined as any;
      this.container.removeChild(this.sprite);
      this.sprite.destroy();
      this.sprite = null;
    }

    const s = new PIXI.AnimatedSprite(textures);
    s.animationSpeed = this.getFPS(name) / 60;
    s.loop = !onComplete;

    if (onComplete) {
      s.onComplete = () => {
        // Chỉ chạy nếu chưa có build() nào khác chen vào
        if (this._gen === myGen) onComplete();
      };
    }

    s.scale.set(this._scale);
    this.container.addChild(s);
    s.play();
    this.sprite = s;
    this._cur   = name;
    this._applyFacing();
  }

  /**
   * Loop animation. Nếu cùng tên đang chạy thì skip.
   * Dùng cho enterState() — các behavior bình thường.
   */
  play(name: string) {
    if (this._cur === name && this.sprite?.playing) return;
    this.build(name);
  }

  /**
   * Chạy 1 lần rồi callback. Override animation hiện tại.
   * Callback tự động bị huỷ nếu có animation khác được build trước khi nó xong.
   */
  playOnce(name: string, onDone: () => void) {
    this._cur = ''; // reset để play() tiếp theo không bị skip
    this.build(name, onDone);
  }

  /**
   * Dừng lại ở frame hiện tại (dùng sau Death để nằm im).
   */
  freeze() {
    this.sprite?.stop();
  }

  private _applyFacing() {
    if (!this.sprite) return;
    this.sprite.scale.x = this._left ? -this._scale : this._scale;
    this.sprite.x       = this._left ? this._size : 0;
  }

  get facingLeft()           { return this._left; }
  set facingLeft(v: boolean) {
    if (this._left === v) return;
    this._left = v;
    this._applyFacing();
  }

  setPosition(x: number, y: number) {
    this.container.x = x;
    this.container.y = y;
  }

  getHeadY(): number {
    if (!this.sprite) return this.container.y;
    const trimY = (this.sprite.texture as any).trim?.y ?? 0;
    return this.container.y + trimY * this._scale;
  }

  get currentAnim() { return this._cur; }
  get animNames()   { return this.tags.map(t => t.name); }

  destroy() {
    this.sprite?.stop();
    this.container.destroy({ children: true });
    this.sheet?.destroy();
  }
}
