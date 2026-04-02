import * as PIXI from 'pixi.js';

export class PetSprite {
  private jsonUrl: string;
  private imgUrl:  string;
  private fpsMap:  Record<string, number>;

  private sheet:   PIXI.Spritesheet | null = null;
  private tags:    { name: string; from: number; to: number }[] = [];
  private allFrameKeys: string[] = []; // ["Frog 0.aseprite", "Frog 1.aseprite", ...]

  private sprite:  PIXI.AnimatedSprite | null = null;
  private _size    = 0;
  private _scale   = 1;
  private _cur     = '';
  private _left    = false;

  public container = new PIXI.Container();

  constructor(
    jsonUrl: string,
    imgUrl:  string,
    fpsMap:  Record<string, number> = {},
  ) {
    this.jsonUrl = jsonUrl;
    this.imgUrl  = imgUrl;
    this.fpsMap  = fpsMap;
  }

  async load(app: PIXI.Application, renderSize: number) {
    this._size  = renderSize;
    this._scale = renderSize / 48;

    // Load JSON raw
    const res      = await fetch(this.jsonUrl);
    const jsonData = await res.json();

    // Load base texture
    const baseTex = await PIXI.Assets.load(this.imgUrl);

    // Lưu frame tags + key order
    this.tags           = jsonData.meta.frameTags;
    this.allFrameKeys   = Object.keys(jsonData.frames);

    // Dùng PIXI.Spritesheet để parse — nó handle trimmed/packed đúng chuẩn
    this.sheet = new PIXI.Spritesheet(baseTex, jsonData);
    await this.sheet.parse();

    console.log(`[PetSprite] loaded ${this.allFrameKeys.length} frames`);
    this.tags.forEach(t =>
      console.log(`  "${t.name}": frame ${t.from}–${t.to}`)
    );

    app.stage.addChild(this.container);
  }

  private getTextures(animName: string): PIXI.Texture[] {
    const tag = this.tags.find(
      t => t.name.toLowerCase() === animName.toLowerCase()
    );
    if (!tag) {
      console.warn(`[PetSprite] unknown: "${animName}". Available: ${this.tags.map(t => t.name).join(', ')}`);
      return [];
    }

    const textures: PIXI.Texture[] = [];
    for (let i = tag.from; i <= tag.to; i++) {
      const key = this.allFrameKeys[i];
      const tex = this.sheet!.textures[key];
      if (tex) textures.push(tex);
    }
    return textures;
  }

  private getFPS(animName: string): number {
    if (this.fpsMap[animName]) return this.fpsMap[animName];
    // Lấy duration từ frame đầu của animation (ms → fps)
    const tag = this.tags.find(t => t.name.toLowerCase() === animName.toLowerCase());
    if (!tag) return 8;
    // sheet.data.frames là object, lấy frame đầu
    const firstKey = this.allFrameKeys[tag.from];
    const dur      = (this.sheet as any).data?.frames?.[firstKey]?.duration ?? 100;
    return Math.round(1000 / dur);
  }

  private build(name: string, onComplete?: () => void) {
    const textures = this.getTextures(name);
    if (!textures.length) return;

    if (this.sprite) {
      this.sprite.stop();
      this.container.removeChild(this.sprite);
      this.sprite.destroy();
      this.sprite = null;
    }

    const s          = new PIXI.AnimatedSprite(textures);
    s.animationSpeed = this.getFPS(name) / 60;
    s.loop           = !onComplete;
    // PIXI.Spritesheet đã handle trim/offset trong texture
    // Chỉ cần scale up là đúng
    s.scale.set(this._scale);
    if (onComplete) s.onComplete = onComplete;

    this.container.addChild(s);
    s.play();
    this.sprite = s;
    this._cur   = name;
    this._applyFacing();
  }

  play(name: string) {
    if (this._cur === name && this.sprite) return;
    this.build(name);
  }

  playOnce(name: string, onDone: () => void) {
    this._cur = '';
    this.build(name, onDone);
  }

  get facingLeft()           { return this._left; }
  set facingLeft(v: boolean) {
    if (this._left === v) return;
    this._left = v;
    this._applyFacing();
  }

  private _applyFacing() {
    if (!this.sprite) return;
    this.sprite.scale.x = this._left ? -this._scale : this._scale;
    this.sprite.x       = this._left ? this._size : 0;
  }

  setPosition(x: number, y: number) {
    this.container.x = x;
    this.container.y = y;
  }

  /**
   * Trả về Y tuyệt đối (stage coords) của top pixel có màu trong frame hiện tại.
   * Dùng texture.trim từ PIXI.Spritesheet — trim.y là offset của trimmed rect
   * trong source 48x48 cell. Không cần scan pixel, rất nhanh.
   */
  getHeadY(): number {
    if (!this.sprite) return this.container.y;

    const tex     = this.sprite.texture;
    // trim.y = khoảng cách từ top source cell đến top của trimmed rect (px trong 48x48)
    const trimY   = (tex as any).trim?.y ?? 0;

    // Scale lên rồi cộng với vị trí container
    return this.container.y + trimY * this._scale;
  }

  get animNames()   { return this.tags.map(t => t.name); }
  get currentAnim() { return this._cur; }

  destroy() {
    this.sprite?.stop();
    this.container.destroy({ children: true });
    this.sheet?.destroy();
  }
}
