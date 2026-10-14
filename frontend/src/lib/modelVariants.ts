import type { Selection } from "./api";

// Инверсия backend component_selection: id варианта -> (слот active.json, значение). Нужна, чтобы
// показать в дропдауне РЕАЛЬНО активный вариант (а не «первый установленный»), совпадая с тем, что
// резолвится при генерации.
const VARIANT_SLOT: Record<string, [string, string]> = {
  higgs: ["tts", "q8_0"], "higgs-q6_k": ["tts", "q6_k"], "higgs-q4_k_m": ["tts", "q4_k_m"],
  parakeet: ["asr", "int8"],
  "parakeet-ultra-int8": ["asr", "ultra-int8"],
  gemma: ["mt", "q4_0"], "gemma-q5_0": ["mt", "q5_0"], "gemma-q6_k": ["mt", "q6_k"], "gemma-q8_0": ["mt", "q8_0"],
  roformer: ["sep", "Q8_0"], "roformer-q5": ["sep", "Q5_0"], "roformer-q4": ["sep", "Q4_0"],
  "whisper-tiny": ["whisper_model", "tiny"], "whisper-base": ["whisper_model", "base"],
  "whisper-small": ["whisper_model", "small"], "whisper-medium": ["whisper_model", "medium"],
  "whisper-large-v3": ["whisper_model", "large-v3"], "whisper-large-v3-turbo": ["whisper_model", "large-v3-turbo"],
};
// Какой из ids сейчас активен по выбору (active.json из capabilities.selection).
export const activeVariantId = (ids: string[], sel: Selection): string | undefined =>
  ids.find((id) => { const m = VARIANT_SLOT[id]; return !!m && sel[m[0]] === m[1]; });

export type VariantI18n = Record<string, { label: "asrVariant.int8" | "asrVariant.fp32" | "asrVariant.ultra" | "asrVariant.ultraInt8"; hint: "asrVariant.int8Hint" | "asrVariant.fp32Hint" | "asrVariant.ultraHint" | "asrVariant.ultraInt8Hint" }>;
// Варианты Parakeet: int8 и fp32 — базовая модель NVIDIA, Ultra — её дообученная Moondream версия.
export const ASR_VARIANT_I18N: VariantI18n = {
  parakeet: { label: "asrVariant.int8", hint: "asrVariant.int8Hint" },
  "parakeet-ultra-int8": { label: "asrVariant.ultraInt8", hint: "asrVariant.ultraInt8Hint" },
};
