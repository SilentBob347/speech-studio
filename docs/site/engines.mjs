// Speech engines as measured on 2026-10-20: Speech Studio 0.2.0 as installed, RTX 4090 (24 GB) under Windows 11, the
// same 17-second Russian text, the second run of each engine (its model loaded). speed: audio seconds per second of
// synthesis; firstMs: time to the first sound in the window; vramGB: peak while speaking less the desktop's own use;
// diskGB: the model as downloaded (Qwen3-TTS Q4_K_M and Higgs Q8_0, the defaults).
export const ENGINE_BENCH = [
  { engine: 'supertonic', speed: 168, firstMs: 103, stream: 'phrase', diskGB: 0.1, vramGB: 1.0 },
  { engine: 'qwen3-tts', speed: 12, firstMs: 19, stream: 'yes', diskGB: 2.2, vramGB: 3.1 },
  { engine: 'qwen3-tts-ru', speed: 10, firstMs: 26, stream: 'yes', diskGB: 2.3, vramGB: 3.5 },
  { engine: 'drama3', speed: 7.0, firstMs: 326, stream: 'yes', diskGB: 21.4, vramGB: 21.9 },
  { engine: 'higgs', speed: 2.3, firstMs: 1023, stream: 'yes', diskGB: 5.3, vramGB: 18.1 },
];
