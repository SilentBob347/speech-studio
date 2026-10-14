import type { Allow } from "./scanHardcoded";

export const ALLOW: Allow = {
  tokens: new Set<string>([
    "OpenRouter", "top-k", "top-p", "GitHub", "Telegram", "Claude Code", "Nerual Dreming", "ArtGeneration.me", "Нейро-Софт",
    "Parakeet-TDT", "Higgs Audio v3", "Hugging Face", "HTTP", "HTTPS", "SOCKS5", "SOCKS4", "Whisper", "GPU", "VRAM", "RAM", "VRAM ·",
    "v",
    "Content-Type", "Aa",
    "Bahasa Indonesia", "Bahasa Melayu", "Basa Jawa", "Basa Sunda",
  ]),
  cyrillicFiles: new Set(["lib/i18n.ts"]),
  // lib/mcpBridge.ts speaks only to the agent over MCP, in English: none of its text reaches the UI.
  skipFiles: new Set(["i18next.d.ts", "lib/mcpBridge.ts"]),
};

/**
 * Values that legitimately equal the English text: brands, units, abbreviations and words that are
 * spelled the same in the language. "*" applies to every language.
 */
export const SAME_AS_EN: Record<"*" | "ru" | "zh" | "es" | "pt" | "fr", string[]> = {
  "*": ["speech.takeWer", "modes.speech", "modes.listening", "app.name", "asrVariant.int8", "asrVariant.fp32", "asrVariant.ultra", "asrVariant.ultraInt8", "jobs.atStage", "downloads.speed", "providers.localGemma", "providers.serverUrlPlaceholder", "url.qHeight", "url.tool.version", "dict.kind_regex", "book.minutes", "book.hours"],
  ru: [],
  zh: ["units.gb", "units.mb", "units.kb", "units.b"],
  es: [
    "tags.rate",    "speech.strictness_normal", "speech.pauseAuto", "voices.snr", "voice.g_female", "voice.g_male", "settings.auto", "settings.sec", "common.error", "setup.error", "units.gb", "units.mb", "units.kb", "units.b", "units.watt", "cloud.beta", "hwPreset.ram", "secrets.proxyTitle",
    ],
  pt: [
    "speech.strictness_normal", "speech.pauseAuto", "voices.snr", "voice.g_female", "voice.g_male", "settings.auto", "settings.sec", "units.gb", "units.mb", "units.kb", "units.b", "units.watt", "cloud.beta", "hwPreset.ram", "secrets.proxyTitle",
    ],
  fr: [
    "listen.pause", "listen.position", "listen.engine_openrouter", "listen.fixes", "listen.fixSay", "listen.format",
    "speech.note", "speech.marker_violet",    "tags.rate",    "dict.note",
    "modes.label", "tags.style", "tags.amusement", "tags.confusion", "tags.surprise", "tags.pause", "jobs.kind.export", "speech.langAuto", "speech.pauseAuto", "speech.format", "voices.pause", "voices.age_senior", "voices.source", "voices.src_pack", "voices.photo", "voice.recordStop", "voice.g_female", "settings.vision",
    "settings.auto", "settings.sec", "help.sectionsTitle", "common.pause", "units.watt", "secrets.proxyTitle", "bridge.agent",
    ],
};
