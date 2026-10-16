// The examples on the project page, as the studio exported them (scripts: the demo generator in the work notes).
// GROUPS: one text per language read by every engine; STRESS: the same Russian text without and with the studio's stress marks.
export const ENGINES = {
  "higgs": "Higgs Audio v3",
  "drama3": "Fish Audio Drama 3",
  "supertonic": "Supertonic 3",
  "qwen3-tts": "Qwen3-TTS",
  "qwen3-tts-ru": "Qwen3-TTS + ru stress LoRA"
};

const NAMES = {
  "RU_Male_Goblin_Puchkov": {
    "ru": "Дмитрий Пучков (Гоблин)",
    "*": "Dmitry Puchkov (Goblin)"
  },
  "RU_Male_Livanov_SherlockHolmes": {
    "ru": "Василий Ливанов (Шерлок Холмс)",
    "*": "Vasily Livanov (Sherlock Holmes)"
  },
  "RU_Male_Nagiev": {
    "ru": "Дмитрий Нагиев",
    "*": "Dmitry Nagiev"
  },
  "": {
    "en": "the model's own voice",
    "ru": "собственный голос модели",
    "es": "voz propia del modelo",
    "fr": "voix propre du modèle",
    "pt": "voz própria do modelo",
    "zh": "模型自带声音"
  }
};

/** A voice as a reader knows it: a known person's name in the page's language, a pack voice by its words, a preset by its name. */
export function voiceName(voice, lang) {
  const known = NAMES[voice];
  if (known) return known[lang] ?? known['*'] ?? known.en;
  return voice.replace(/_/g, ' ').replace(/([a-z]{2,})(\d)/gi, '$1 $2').replace(/\b([a-z])/g, (c) => c.toUpperCase());
}

export const GROUPS = [
  {
    "lang": "ru",
    "key": "pushkin",
    "text": "Я помню чудное мгновенье — передо мной явилась ты, как мимолётное виденье, как гений чистой красоты.",
    "clips": [
      {
        "file": "ru-pushkin-higgs-ru-male-goblin-puchkov.mp3",
        "engine": "higgs",
        "voice": "RU_Male_Goblin_Puchkov",
        "kind": "clone"
      },
      {
        "file": "ru-pushkin-drama3-ru-male-livanov-sherlockholmes.mp3",
        "engine": "drama3",
        "voice": "RU_Male_Livanov_SherlockHolmes",
        "kind": "clone"
      },
      {
        "file": "ru-pushkin-qwen3-tts-ru-ru-male-nagiev.mp3",
        "engine": "qwen3-tts-ru",
        "voice": "RU_Male_Nagiev",
        "kind": "clone"
      },
      {
        "file": "ru-pushkin-supertonic-f3.mp3",
        "engine": "supertonic",
        "voice": "F3",
        "kind": "preset"
      },
      {
        "file": "ru-pushkin-qwen3-tts-ryan.mp3",
        "engine": "qwen3-tts",
        "voice": "ryan",
        "kind": "preset"
      }
    ]
  },
  {
    "lang": "ru",
    "key": "numbers",
    "text": "Встреча 12.03 в 18:30, билет стоит 150 руб., а на улице −5 °C.",
    "clips": [
      {
        "file": "ru-numbers-higgs-ru-male-goblin-puchkov.mp3",
        "engine": "higgs",
        "voice": "RU_Male_Goblin_Puchkov",
        "kind": "clone"
      },
      {
        "file": "ru-numbers-drama3-ru-male-livanov-sherlockholmes.mp3",
        "engine": "drama3",
        "voice": "RU_Male_Livanov_SherlockHolmes",
        "kind": "clone"
      },
      {
        "file": "ru-numbers-qwen3-tts-ru-ru-male-nagiev.mp3",
        "engine": "qwen3-tts-ru",
        "voice": "RU_Male_Nagiev",
        "kind": "clone"
      },
      {
        "file": "ru-numbers-supertonic-f3.mp3",
        "engine": "supertonic",
        "voice": "F3",
        "kind": "preset"
      },
      {
        "file": "ru-numbers-qwen3-tts-ryan.mp3",
        "engine": "qwen3-tts",
        "voice": "ryan",
        "kind": "preset"
      }
    ]
  },
  {
    "lang": "en",
    "key": "sonnet18",
    "text": "Shall I compare thee to a summer's day? Thou art more lovely and more temperate: rough winds do shake the darling buds of May, and summer's lease hath all too short a date.",
    "clips": [
      {
        "file": "en-sonnet18-higgs-english-female2.mp3",
        "engine": "higgs",
        "voice": "English_Female2",
        "kind": "clone"
      },
      {
        "file": "en-sonnet18-drama3-english-female.mp3",
        "engine": "drama3",
        "voice": "English_Female",
        "kind": "clone"
      },
      {
        "file": "en-sonnet18-supertonic-m2.mp3",
        "engine": "supertonic",
        "voice": "M2",
        "kind": "preset"
      },
      {
        "file": "en-sonnet18-qwen3-tts-ryan.mp3",
        "engine": "qwen3-tts",
        "voice": "ryan",
        "kind": "preset"
      }
    ]
  },
  {
    "lang": "es",
    "key": "becquer",
    "text": "¿Qué es poesía?, dices mientras clavas en mi pupila tu pupila azul. ¿Qué es poesía? ¿Y tú me lo preguntas? Poesía… eres tú.",
    "clips": [
      {
        "file": "es-becquer-higgs-spanish-male.mp3",
        "engine": "higgs",
        "voice": "Spanish_Male",
        "kind": "clone"
      },
      {
        "file": "es-becquer-drama3-spanish-male.mp3",
        "engine": "drama3",
        "voice": "Spanish_Male",
        "kind": "clone"
      },
      {
        "file": "es-becquer-supertonic-f2.mp3",
        "engine": "supertonic",
        "voice": "F2",
        "kind": "preset"
      },
      {
        "file": "es-becquer-qwen3-tts-serena.mp3",
        "engine": "qwen3-tts",
        "voice": "serena",
        "kind": "preset"
      }
    ]
  },
  {
    "lang": "fr",
    "key": "verlaine",
    "text": "Il pleure dans mon cœur comme il pleut sur la ville ; quelle est cette langueur qui pénètre mon cœur ?",
    "clips": [
      {
        "file": "fr-verlaine-higgs-french-female.mp3",
        "engine": "higgs",
        "voice": "French_Female",
        "kind": "clone"
      },
      {
        "file": "fr-verlaine-drama3-french-female.mp3",
        "engine": "drama3",
        "voice": "French_Female",
        "kind": "clone"
      },
      {
        "file": "fr-verlaine-supertonic-f4.mp3",
        "engine": "supertonic",
        "voice": "F4",
        "kind": "preset"
      },
      {
        "file": "fr-verlaine-qwen3-tts-vivian.mp3",
        "engine": "qwen3-tts",
        "voice": "vivian",
        "kind": "preset"
      }
    ]
  },
  {
    "lang": "pt",
    "key": "camoes",
    "text": "Amor é fogo que arde sem se ver, é ferida que dói, e não se sente; é um contentamento descontente, é dor que desatina sem doer.",
    "clips": [
      {
        "file": "pt-camoes-higgs-base.mp3",
        "engine": "higgs",
        "voice": "",
        "kind": "preset"
      },
      {
        "file": "pt-camoes-supertonic-m3.mp3",
        "engine": "supertonic",
        "voice": "M3",
        "kind": "preset"
      },
      {
        "file": "pt-camoes-qwen3-tts-dylan.mp3",
        "engine": "qwen3-tts",
        "voice": "dylan",
        "kind": "preset"
      }
    ]
  },
  {
    "lang": "zh",
    "key": "libai",
    "text": "床前明月光，疑是地上霜。举头望明月，低头思故乡。",
    "clips": [
      {
        "file": "zh-libai-higgs-base.mp3",
        "engine": "higgs",
        "voice": "",
        "kind": "preset"
      },
      {
        "file": "zh-libai-drama3-english-female.mp3",
        "engine": "drama3",
        "voice": "English_Female",
        "kind": "clone"
      },
      {
        "file": "zh-libai-qwen3-tts-vivian.mp3",
        "engine": "qwen3-tts",
        "voice": "vivian",
        "kind": "preset"
      },
      {
        "file": "zh-libai-qwen3-tts-dylan.mp3",
        "engine": "qwen3-tts",
        "voice": "dylan",
        "kind": "preset"
      }
    ]
  }
];

export const STRESS = [
  {
    "key": "homographs",
    "text": "Старинный замок стоял на холме, а дверь в него запирали на тяжёлый замок. Из этой муки пекут хлеб, но для пекаря это настоящие муки.",
    "pairs": [
      {
        "engine": "supertonic",
        "voice": "F3",
        "kind": "preset",
        "before": "ru-homographs-supertonic-before.mp3",
        "after": "ru-homographs-supertonic-after.mp3"
      },
      {
        "engine": "qwen3-tts-ru",
        "voice": "RU_Male_Nagiev",
        "kind": "clone",
        "before": "ru-homographs-qwen3-tts-ru-before.mp3",
        "after": "ru-homographs-qwen3-tts-ru-after.mp3"
      }
    ]
  },
  {
    "key": "homographs2",
    "text": "Мы долго стояли у берега и смотрели, как белки прыгают по веткам. В яйце много белка, а в этой задаче много лишних белков.",
    "pairs": [
      {
        "engine": "supertonic",
        "voice": "F3",
        "kind": "preset",
        "before": "ru-homographs2-supertonic-before.mp3",
        "after": "ru-homographs2-supertonic-after.mp3"
      },
      {
        "engine": "qwen3-tts-ru",
        "voice": "RU_Male_Nagiev",
        "kind": "clone",
        "before": "ru-homographs2-qwen3-tts-ru-before.mp3",
        "after": "ru-homographs2-qwen3-tts-ru-after.mp3"
      }
    ]
  }
];
