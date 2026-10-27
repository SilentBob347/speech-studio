import type { Selection } from "./api";

// Инверсия backend component_selection: id варианта -> (слот active.json, значение). Нужна, чтобы
// показать в дропдауне РЕАЛЬНО активный вариант (а не «первый установленный»), совпадая с тем, что
// резолвится при генерации.
const VARIANT_SLOT: Record<string, [string, string]> = {
  higgs: ["tts", "q8_0"], "higgs-q6_k": ["tts", "q6_k"], "higgs-q4_k_m": ["tts", "q4_k_m"],
  "qwen3-tts": ["qwen_tts_quant", "Q4_K_M"], "qwen3-tts-q8_0": ["qwen_tts_quant", "Q8_0"],
  parakeet: ["asr", "int8"],
  "parakeet-ultra": ["asr", "ultra-int8"],
  gemma: ["mt", "q4_0"], "gemma-q5_0": ["mt", "q5_0"], "gemma-q6_k": ["mt", "q6_k"], "gemma-q8_0": ["mt", "q8_0"],
  roformer: ["sep", "Q8_0"], "roformer-q5": ["sep", "Q5_0"], "roformer-q4": ["sep", "Q4_0"],
};
// Какой из ids сейчас активен по выбору (active.json из capabilities.selection).
export const activeVariantId = (ids: string[], sel: Selection): string | undefined =>
  ids.find((id) => { const m = VARIANT_SLOT[id]; return !!m && sel[m[0]] === m[1]; });

export type VariantI18n = Record<string, { label: "asrVariant.int8" | "asrVariant.ultraInt8"; hint: "asrVariant.int8Hint" | "asrVariant.ultraInt8Hint" }>;
// Варианты Parakeet (GGUF Q8_0 на transcribe.cpp): базовая модель NVIDIA и Ultra — её дообученная Moondream версия.
export const ASR_VARIANT_I18N: VariantI18n = {
  parakeet: { label: "asrVariant.int8", hint: "asrVariant.int8Hint" },
  "parakeet-ultra": { label: "asrVariant.ultraInt8", hint: "asrVariant.ultraInt8Hint" },
};
