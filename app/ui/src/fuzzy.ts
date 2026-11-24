// Fuzzy matcher for the quick switcher and the command palette. Pure, no DOM.

export interface FuzzyItem {
  /** Text shown and matched (vault-relative path, command title…). */
  text: string;
  /** Lower-cased copy of `text`, computed once. */
  lower: string;
  /** Index where the basename starts in `text` (0 when there is no folder part). */
  baseStart: number;
}

export interface FuzzyHit<T extends FuzzyItem> {
  item: T;
  score: number;
  /** Matched character indexes in `item.text`, ascending. */
  positions: number[];
}

export function makeItem(text: string): FuzzyItem {
  return { text, lower: text.toLowerCase(), baseStart: text.lastIndexOf("/") + 1 };
}

function isBoundary(lower: string, i: number): boolean {
  if (i === 0) return true;
  const c = lower.charCodeAt(i - 1);
  // space - _ / . ( [
  return c === 32 || c === 45 || c === 95 || c === 47 || c === 46 || c === 40 || c === 91;
}

/**
 * Scores `query` (already lower-cased, no spaces) against one item.
 * Returns -1 when the query is not a subsequence. Greedy from the basename first, then from the start.
 */
export function scoreItem(q: string, item: FuzzyItem, positions?: number[]): number {
  const s = item.lower;
  const n = q.length;
  if (n === 0) return 0;
  if (n > s.length) return -1;
  // Contiguous match is by far the most common intent: find it first, prefer the basename.
  let idx = s.indexOf(q, item.baseStart);
  if (idx < 0) idx = s.indexOf(q);
  if (idx >= 0) {
    let score = 100 + n * 8;
    if (idx >= item.baseStart) score += 40;
    if (isBoundary(s, idx)) score += 30;
    if (idx === item.baseStart) score += 20;
    // Shorter names are closer matches; the folder part only counts when the match is in it.
    score -= idx >= item.baseStart ? Math.min(30, s.length - item.baseStart - n) : Math.min(30, (s.length - n) >> 1);
    if (positions) for (let k = 0; k < n; k++) positions.push(idx + k);
    return score;
  }
  // Subsequence match, tried from the basename then from the beginning.
  const from = subsequenceFrom(q, s, item.baseStart) ? item.baseStart : subsequenceFrom(q, s, 0) ? 0 : -1;
  if (from < 0) return -1;
  let score = from > 0 || item.baseStart === 0 ? 20 : 0;
  let si = from;
  let prev = -2;
  for (let qi = 0; qi < n; qi++) {
    const c = q[qi];
    let at = s.indexOf(c, si);
    // Prefer a word-boundary occurrence a little further when the direct one is mid-word.
    if (at !== prev + 1 && !isBoundary(s, at)) {
      let look = at;
      for (let tries = 0; tries < 4 && look >= 0; tries++) {
        look = s.indexOf(c, look + 1);
        if (look >= 0 && isBoundary(s, look) && subsequenceFrom(q.slice(qi + 1), s, look + 1)) {
          at = look;
          break;
        }
      }
    }
    if (at === prev + 1) score += 8;
    else if (isBoundary(s, at)) score += 10;
    else score += 1;
    if (prev >= 0) score -= Math.min(6, at - prev - 1);
    if (positions) positions.push(at);
    prev = at;
    si = at + 1;
  }
  score -= Math.min(30, (s.length - n) >> 2);
  return score;
}

function subsequenceFrom(q: string, s: string, from: number): boolean {
  let si = from;
  for (let qi = 0; qi < q.length; qi++) {
    si = s.indexOf(q[qi], si);
    if (si < 0) return false;
    si++;
  }
  return true;
}

/** Returns the best `limit` hits. Space-separated terms must all match (scores add up). */
export function fuzzySearch<T extends FuzzyItem>(query: string, items: readonly T[], limit = 50): FuzzyHit<T>[] {
  const terms = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (!terms.length) return items.slice(0, limit).map((item) => ({ item, score: 0, positions: [] }));
  const top: { item: T; score: number }[] = [];
  let min = -Infinity;
  for (let i = 0; i < items.length; i++) {
    const item = items[i];
    let total = 0;
    for (let t = 0; t < terms.length; t++) {
      const sc = scoreItem(terms[t], item);
      if (sc < 0) {
        total = -1;
        break;
      }
      total += sc;
    }
    if (total < 0) continue;
    if (top.length < limit) {
      top.push({ item, score: total });
      if (top.length === limit) {
        top.sort((a, b) => b.score - a.score);
        min = top[limit - 1].score;
      }
    } else if (total > min) {
      // Insert in order, drop the last.
      let j = limit - 1;
      while (j > 0 && top[j - 1].score < total) {
        top[j] = top[j - 1];
        j--;
      }
      top[j] = { item, score: total };
      min = top[limit - 1].score;
    }
  }
  if (top.length < limit) top.sort((a, b) => b.score - a.score);
  return top.map(({ item, score }) => {
    const positions: number[] = [];
    for (const t of terms) scoreItem(t, item, positions);
    positions.sort((a, b) => a - b);
    return { item, score, positions };
  });
}
