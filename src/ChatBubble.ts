import * as PIXI from 'pixi.js';

export class ChatBubble {
  private container = new PIXI.Container();
  private text: PIXI.Text;

  private hideTimer: ReturnType<typeof setTimeout> | null = null;
  private _visible = false;

  constructor(private stage: PIXI.Container) {
    this.container.visible = false;
    this.container.sortableChildren = false;

    this.text = new PIXI.Text({
      text: '',
      style: {
        fontFamily: '"Press Start 2P"',
        fontSize: 11,
        fill: 0xffffff,
        stroke: {
          color: 0x000000,
          width: 5,      // Viền dày 5px là rất to so với chữ 11px
          join: 'round',
        },
        // --- THÊM DÒNG NÀY LÀ HẾT BỊ CẮT ---
        padding: 10,     // Chừa ra 10px xung quanh để vẽ stroke thoải mái
        // ----------------------------------
        align: 'center',
        wordWrap: true,
        wordWrapWidth: 120, // Tăng cái này lên tí để stroke không làm chữ bị xuống dòng bậy
      },
      resolution: 2, // Để 2 cho nó nét, 1 nhìn viền 5px nó vỡ lắm
    });

    // Ép toạ độ về số nguyên để không bị lệch nửa pixel gây mờ chữ
    this.text.roundPixels = true;

    this.container.addChild(this.text);
    stage.addChild(this.container);
  }

  // Hàm load rỗng để bác không phải sửa code ở các file khác gọi đến nó
  async load() { }

  showText(message: string, centerX: number, headY: number, durationMs = 2800) {
    this.text.text = message;

    // Căn giữa chữ dựa trên toạ độ centerX truyền vào
    this.text.x = -Math.floor(this.text.width / 2);

    this._visible = true;
    this.container.visible = true;

    this._setPosition(centerX, headY);
    this._scheduleHide(durationMs);
  }

  showMood(mood: string, centerX: number, headY: number, durationMs = 2800) {
    const moodMap: Record<string, string> = {
      idle: 'Ribbit',
      happy: 'Yay!',
      bored: 'So\nbored',
      hungry: 'Gimme\nbugs!',
      scared: 'Yikes!',
      sleep: 'Zzz..',
      angry: 'Grrr!',
    };

    const msg = moodMap[mood] || '...';
    this.showText(msg, centerX, headY, durationMs);
  }

  private _setPosition(centerX: number, headY: number) {
    // Đặt container vào đúng vị trí centerX
    this.container.x = Math.round(centerX);
    // Cho chữ bay trên đầu một khoảng tầm 10px
    this.container.y = Math.round(headY - this.text.height - 10);
  }

  updatePosition(centerX: number, headY: number) {
    if (!this._visible) return;
    this._setPosition(centerX, headY);
  }

  hide() {
    if (this.hideTimer) { clearTimeout(this.hideTimer); this.hideTimer = null; }
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
  }
}
