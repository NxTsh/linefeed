// Visual-line (rendered row) grouping. Real scripts put whole paragraphs on
// one source line; CSS soft-wraps them. Every positioning decision
// (highlight, anchor, lookahead) is row-granular, so words are regrouped by
// their MEASURED y. This module is pure; prompter.ts feeds it a batched
// measurement pass (the only place layout is read).

export interface WordMeasure {
  /** Word index in the flat payload order. */
  idx: number;
  /** offsetTop of the word span. */
  y: number;
  /** offsetHeight of the word span. */
  h: number;
  /** Paragraph index — paragraph boundaries force a new row even when the
   * measured y jitter would merge them. */
  para: number;
}

export interface VisualRow {
  row: number;
  top: number;
  height: number;
  firstWord: number;
  lastWord: number;
}

/** Row-merge tolerance: 30% of the median row pitch, clamped to [1, 12] px
 * (sub-pixel layout jitter merges; distinct rows never do). */
export function visualTolerance(pitches: number[]): number {
  if (pitches.length === 0) return 4;
  const sorted = [...pitches].sort((a, b) => a - b);
  const median = sorted[sorted.length >> 1]!;
  return Math.max(1, Math.min(12, 0.3 * median));
}

/** Group measured words into visual rows. Words must be in document order. */
export function groupVisualRows(measures: WordMeasure[], tol: number): VisualRow[] {
  const rows: VisualRow[] = [];
  let cur: VisualRow | null = null;
  let curPara = -1;
  for (const m of measures) {
    const sameRow =
      cur !== null && m.para === curPara && Math.abs(m.y - cur.top) <= tol;
    if (sameRow && cur) {
      cur.lastWord = m.idx;
      cur.height = Math.max(cur.height, m.y + m.h - cur.top);
      cur.top = Math.min(cur.top, m.y);
    } else {
      cur = {
        row: rows.length,
        top: m.y,
        height: m.h,
        firstWord: m.idx,
        lastWord: m.idx,
      };
      curPara = m.para;
      rows.push(cur);
    }
  }
  return rows;
}

/** Word index → row index lookup table. */
export function rowOfWordTable(rows: VisualRow[], nWords: number): number[] {
  const table = new Array<number>(nWords).fill(0);
  for (const r of rows) {
    for (let w = r.firstWord; w <= r.lastWord && w < nWords; w++) {
      table[w] = r.row;
    }
  }
  return table;
}

/** Median row pitch (top-to-top distance), for the tolerance and for
 * anchor math when only one row exists (falls back to row height). */
export function rowPitch(rows: VisualRow[]): number {
  if (rows.length === 0) return 0;
  if (rows.length === 1) return rows[0]!.height;
  const pitches: number[] = [];
  for (let i = 1; i < rows.length; i++) {
    pitches.push(rows[i]!.top - rows[i - 1]!.top);
  }
  const sorted = pitches.sort((a, b) => a - b);
  return sorted[sorted.length >> 1]!;
}
