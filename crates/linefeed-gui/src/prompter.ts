// The prompter view: builds the word DOM, measures visual rows (the ONLY
// layout-read site), runs the spring rAF loop, applies config (font, zone,
// mirror), and handles manual scrolling.
//
// Perf contract: layout is re-measured only when `layoutDirty` — script
// load, font/zone change, resize, fonts ready. Never per cursor event.

import {
  anchorY,
  currentWordIndex,
  intraRowFraction,
  mirrorTransform,
  scrollTarget,
  READING_FONTS,
} from "./pipeline.ts";
import type { GuiConfig, ScriptPayload } from "./types.ts";
import { settled, springStep, type SpringState } from "./spring.ts";
import {
  effectiveTarget,
  followInit,
  followResume,
  followSuspend,
  followWheel,
  type FollowState,
} from "./follow.ts";
import {
  groupVisualRows,
  rowOfWordTable,
  rowPitch,
  visualTolerance,
  type VisualRow,
  type WordMeasure,
} from "./visual.ts";

export class PrompterView {
  private zone: HTMLElement;
  private column: HTMLElement;
  private anchorEl: HTMLElement;
  private rowBar: HTMLElement;
  private emptyEl: HTMLElement;

  private script: ScriptPayload | null = null;
  private wordEls: HTMLElement[] = [];
  private rows: VisualRow[] = [];
  private rowTable: number[] = [];
  private pitch = 0;
  private anchor = 0;

  private spring: SpringState = { pos: 0, vel: 0 };
  private target = 0;
  private rafActive = false;
  private layoutDirty = true;

  private cursor = 0;
  private currentRow = -1;
  follow: FollowState = followInit();
  private cfg: GuiConfig | null = null;

  onFollowChange: (suspended: boolean) => void = () => {};

  constructor(root: Document = document) {
    this.zone = root.getElementById("zone")!;
    this.column = root.getElementById("column")!;
    this.anchorEl = root.getElementById("anchor")!;
    this.rowBar = root.getElementById("row-bar")!;
    this.emptyEl = root.getElementById("empty")!;

    this.zone.addEventListener(
      "wheel",
      (e) => {
        e.preventDefault();
        this.manualScroll(e.deltaY);
      },
      { passive: false },
    );
  }

  setScript(script: ScriptPayload): void {
    this.script = script;
    this.cursor = 0;
    this.currentRow = -1;
    this.follow = followInit();
    this.onFollowChange(false);
    this.spring = { pos: 0, vel: 0 };
    this.buildDom();
    this.layoutDirty = true;
    this.reanchor();
    this.emptyEl.classList.add("hidden");
  }

  applyConfig(cfg: GuiConfig): void {
    const prev = this.cfg;
    this.cfg = cfg;
    const font = READING_FONTS.find((f) => f.id === cfg.reading_font) ?? READING_FONTS[0]!;
    this.column.style.fontFamily = font.css;
    this.column.style.fontSize = `${cfg.font_px}px`;
    this.zone.style.width = `${cfg.reading_width}%`;
    this.zone.style.height = `${cfg.reading_height}%`;
    const layoutChanged =
      !prev ||
      prev.font_px !== cfg.font_px ||
      prev.reading_font !== cfg.reading_font ||
      prev.reading_width !== cfg.reading_width ||
      prev.reading_height !== cfg.reading_height;
    if (layoutChanged) {
      this.layoutDirty = true;
      this.reanchor();
    } else {
      this.applyTransform();
    }
  }

  /** Re-measure rows and re-derive the anchor. Call on resize/fonts-ready. */
  reanchor(): void {
    if (!this.script || this.wordEls.length === 0) {
      this.anchorEl.style.top = "33.333%";
      return;
    }
    if (this.layoutDirty) {
      this.measure();
      this.layoutDirty = false;
    }
    const zoneH = this.zone.clientHeight;
    this.anchor = anchorY(zoneH, this.pitch || 1);
    // The visible guide tracks the COMPUTED anchor.
    this.anchorEl.style.top = `${this.anchor}px`;
    this.updateTarget(true);
  }

  markLayoutDirty(): void {
    this.layoutDirty = true;
  }

  setCursor(cursor: number): void {
    this.cursor = cursor;
    if (!this.script) return;
    const wi = currentWordIndex(this.script.words, cursor);
    const row = wi >= 0 ? (this.rowTable[wi] ?? 0) : -1;
    if (row !== this.currentRow) {
      this.currentRow = row;
      this.paintRow(row, wi);
      // Bolding the current row can reflow rows below it.
      this.layoutDirty = true;
      if (this.layoutDirty) {
        this.measure();
        this.layoutDirty = false;
      }
    } else {
      this.paintWords(wi);
    }
    this.updateTarget(false);
  }

  resumeFollow(): void {
    this.follow = followResume(this.follow);
    this.onFollowChange(false);
    this.updateTarget(true);
  }

  private manualScroll(deltaY: number): void {
    if (!this.script) return;
    if (!this.follow.suspended) {
      this.follow = followSuspend(this.follow, this.spring.pos);
      this.onFollowChange(true);
    }
    this.follow = followWheel(this.follow, deltaY, this.maxTarget());
    this.updateTarget(false);
  }

  private maxTarget(): number {
    return Math.max(0, this.column.scrollHeight - this.zone.clientHeight);
  }

  private updateTarget(jump: boolean): void {
    if (!this.script) return;
    let auto = this.target;
    if (this.currentRow >= 0 && this.rows.length > 0) {
      const frac = intraRowFraction(
        this.script.words,
        (w) => this.rowTable[w] ?? 0,
        this.cursor,
      );
      auto = scrollTarget(
        this.rows.map((r) => r.top),
        this.currentRow,
        frac,
        this.cfg?.lead_lines ?? 1,
        this.anchor,
        this.maxTarget(),
      );
    } else if (this.currentRow < 0) {
      auto = 0;
    }
    this.target = effectiveTarget(this.follow, auto);
    if (jump) {
      this.spring = { pos: this.target, vel: 0 };
      this.applyTransform();
    } else {
      this.kickRaf();
    }
  }

  private kickRaf(): void {
    if (this.rafActive) return;
    this.rafActive = true;
    const tick = () => {
      this.spring = springStep(this.spring, this.target, 1 / 60);
      this.applyTransform();
      if (settled(this.spring, this.target)) {
        this.spring = { pos: this.target, vel: 0 };
        this.applyTransform();
        this.rafActive = false;
        return;
      }
      requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
  }

  private applyTransform(): void {
    const h = this.cfg?.mirror_h ?? false;
    const v = this.cfg?.mirror_v ?? false;
    this.column.style.transform = mirrorTransform(h, v, this.spring.pos);
  }

  private paintRow(row: number, wi: number): void {
    if (row < 0 || row >= this.rows.length) {
      this.rowBar.style.opacity = "0";
      this.paintWords(wi);
      return;
    }
    const r = this.rows[row]!;
    this.rowBar.style.opacity = "1";
    this.rowBar.style.top = `${r.top}px`;
    this.rowBar.style.height = `${r.height}px`;
    this.paintWords(wi);
  }

  private paintWords(wi: number): void {
    // Row-granular classes: read rows dim, the current row highlights.
    for (let i = 0; i < this.wordEls.length; i++) {
      const el = this.wordEls[i]!;
      const row = this.rowTable[i] ?? 0;
      const cls =
        this.currentRow >= 0 && row < this.currentRow
          ? "word read"
          : row === this.currentRow && i <= wi
            ? "word current"
            : "word";
      if (el.className !== cls) el.className = cls;
    }
    // The bar follows the row; row position is repainted on row change only.
    if (this.currentRow >= 0 && this.currentRow < this.rows.length) {
      const r = this.rows[this.currentRow]!;
      this.rowBar.style.top = `${r.top}px`;
      this.rowBar.style.height = `${r.height}px`;
    }
  }

  private buildDom(): void {
    this.column.textContent = "";
    this.wordEls = [];
    if (!this.script) return;
    let para = -1;
    let paraEl: HTMLElement | null = null;
    for (const w of this.script.words) {
      if (w.para !== para) {
        para = w.para;
        paraEl = document.createElement("div");
        paraEl.className = "paragraph";
        this.column.appendChild(paraEl);
      }
      const span = document.createElement("span");
      span.className = "word";
      span.textContent = w.raw;
      paraEl!.appendChild(span);
      paraEl!.appendChild(document.createTextNode(" "));
      this.wordEls.push(span);
    }
    // Row bar lives in the zone, positioned in column coordinates: move it
    // with the column by parenting it there.
    this.column.appendChild(this.rowBar);
  }

  /** Batched layout read: offsetTop/offsetHeight per word, one pass. */
  private measure(): void {
    if (!this.script) return;
    const measures: WordMeasure[] = this.wordEls.map((el, i) => ({
      idx: i,
      y: el.offsetTop,
      h: el.offsetHeight,
      para: this.script!.words[i]!.para,
    }));
    const pitches: number[] = [];
    for (let i = 1; i < measures.length; i++) {
      const dy = measures[i]!.y - measures[i - 1]!.y;
      if (dy > 0) pitches.push(dy);
    }
    const tol = visualTolerance(pitches);
    this.rows = groupVisualRows(measures, tol);
    this.rowTable = rowOfWordTable(this.rows, this.wordEls.length);
    this.pitch = rowPitch(this.rows);
  }
}
