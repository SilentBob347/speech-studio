/** A step of the word alignment of what a block says and what recognition heard, as the server's `qc::word_diff`. */
export type WordStep = { op: "same" | "inserted" | "deleted" | "substituted"; expected?: string; heard?: string };

/** Words as the check compares them: no engine tags, pauses or stress marks, lower case, «ё» as «е». */
export function words(text: string): string[] {
  return text
    .replace(/<\|[^|]*\|>|<[^>]*>|\[[^\]]*\]|sil<\[[^\]]*\]>/g, " ")
    .replace(/\+/g, "")
    .toLowerCase()
    .replace(/\u0451/g, () => String.fromCharCode(0x435))
    .split(/[^\p{L}\p{N}']+/u)
    .filter(Boolean);
}

/** Levenshtein alignment of the expected and heard words, in the order of the text. */
export function wordDiff(expected: string[], heard: string[]): WordStep[] {
  const n = expected.length;
  const m = heard.length;
  const d = Array.from({ length: n + 1 }, (_, i) => Array.from({ length: m + 1 }, (_, j) => (i === 0 ? j : j === 0 ? i : 0)));
  for (let i = 1; i <= n; i++) {
    for (let j = 1; j <= m; j++) {
      d[i][j] = Math.min(d[i - 1][j] + 1, d[i][j - 1] + 1, d[i - 1][j - 1] + (expected[i - 1] === heard[j - 1] ? 0 : 1));
    }
  }
  const steps: WordStep[] = [];
  let i = n;
  let j = m;
  while (i > 0 || j > 0) {
    if (i > 0 && j > 0 && d[i][j] === d[i - 1][j - 1] + (expected[i - 1] === heard[j - 1] ? 0 : 1)) {
      steps.push({ op: expected[i - 1] === heard[j - 1] ? "same" : "substituted", expected: expected[i - 1], heard: heard[j - 1] });
      i--;
      j--;
    } else if (i > 0 && d[i][j] === d[i - 1][j] + 1) {
      steps.push({ op: "deleted", expected: expected[i - 1] });
      i--;
    } else {
      steps.push({ op: "inserted", heard: heard[j - 1] });
      j--;
    }
  }
  return steps.reverse();
}
