/** The words of a voice description, by attribute, in English: the voice designer reads English best. */
export const VOICE_ATTRS = {
  gender: { female: "female", male: "male" },
  age: { child: "child", teen: "teenage", young: "young adult", middle: "middle-aged", senior: "elderly" },
  pitch: { low: "low-pitched", mid: "mid-pitched", high: "high-pitched" },
  pace: { slow: "speaks slowly", moderate: "speaks at a moderate pace", fast: "speaks fast" },
  timbre: { warm: "warm", bright: "bright", husky: "husky", breathy: "breathy", deep: "deep", clear: "clear", raspy: "raspy", soft: "soft" },
  manner: { calm: "calm", energetic: "energetic", authoritative: "authoritative", gentle: "gentle", playful: "playful", serious: "serious", cheerful: "cheerful", melancholic: "melancholic" },
} as const;

export type VoiceAttrs = {
  gender: keyof typeof VOICE_ATTRS.gender | "";
  age: keyof typeof VOICE_ATTRS.age | "";
  pitch: keyof typeof VOICE_ATTRS.pitch | "";
  pace: keyof typeof VOICE_ATTRS.pace | "";
  timbre: (keyof typeof VOICE_ATTRS.timbre)[];
  manner: (keyof typeof VOICE_ATTRS.manner)[];
  accent: string;
};

export const NO_ATTRS: VoiceAttrs = { gender: "", age: "", pitch: "", pace: "", timbre: [], manner: [], accent: "" };

const sentence = (s: string) => (s ? `${s.charAt(0).toUpperCase()}${s.slice(1)}.` : "");
const both = (words: readonly string[]) => (words.length > 1 ? `${words.slice(0, -1).join(", ")} and ${words[words.length - 1]}` : words[0] ?? "");

/** The attributes as the designer's description: who speaks, how the voice sounds, how it speaks. */
export function describe(a: VoiceAttrs): string {
  const who = [a.age && VOICE_ATTRS.age[a.age], a.gender && VOICE_ATTRS.gender[a.gender]].filter(Boolean).join(" ");
  const sound = [a.pitch && VOICE_ATTRS.pitch[a.pitch], a.timbre.length ? `with a ${both(a.timbre.map((x) => VOICE_ATTRS.timbre[x]))} timbre` : ""].filter(Boolean).join(", ");
  const first = [`${who ? `a ${who}` : "a"} voice`, sound].filter(Boolean).join(", ");
  const manner = a.manner.length ? `${both(a.manner.map((x) => VOICE_ATTRS.manner[x]))} delivery` : "";
  const second = [manner, a.pace && VOICE_ATTRS.pace[a.pace]].filter(Boolean).join("; ");
  const accent = a.accent.trim() ? `${a.accent.trim()} accent` : "";
  return [sentence(first.replace(/^a ([aeiou])/, "an $1")), sentence(second), sentence(accent)].filter(Boolean).join(" ");
}
