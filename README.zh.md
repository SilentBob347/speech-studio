<div align="center">

<img src="frontend/public/favicon.svg" width="72" alt="Speech Studio 标志"/>

# Speech Studio

**免费、离线的 Windows AI 语音工作室——用克隆声音和内置声音为任意文本配音，把录音转成带说话人的文字。俄语处理到位：数字按格变化、重音、«ё» 与同形异音词。100% 本地运行，零 Python：一个原生 `.exe`（Rust + C++/CUDA）；每个模型和引擎一键下载。**

[![License](https://img.shields.io/github/license/timoncool/speech-studio?style=flat-square)](LICENSE)
[![Stars](https://img.shields.io/github/stars/timoncool/speech-studio?style=flat-square)](https://github.com/timoncool/speech-studio/stargazers)
[![Latest release](https://img.shields.io/github/v/release/timoncool/speech-studio?include_prereleases&style=flat-square)](https://github.com/timoncool/speech-studio/releases)
[![Downloads](https://img.shields.io/github/downloads/timoncool/speech-studio/total?style=flat-square)](https://github.com/timoncool/speech-studio/releases)
[![Last commit](https://img.shields.io/github/last-commit/timoncool/speech-studio?style=flat-square)](https://github.com/timoncool/speech-studio/commits)

[English](README.md) · [Русский](README.ru.md) · **中文** · [Español](README.es.md) · [Português](README.pt.md) · [Français](README.fr.md)

![Speech Studio——配好音的对话，每个角色一个声音](docs/shots/speech-ru.png)

</div>

## 这是什么

**Speech Studio** 有两种模式。**Speech** 为文本配音：一句话、每个角色各有声音的对话、由“导演”为你写好的播客，或者来自 EPUB、FB2、PDF、DOCX 的整本书。**Listening** 把录音、视频或链接转成文字，标明谁在什么时候说了什么，可以按文字剪辑，可以把配音与应有的文本逐词核对，还能从说话人生成声音库里的声音。

一切都**在你的电脑上本地运行**——无云端、无订阅；你的文本、录音和声纹不会离开电脑。如果想用显卡跑不动的声音，也可以使用 **OpenRouter**、**Google Gemini**、**fal.ai** 和 **Runware** 的云端声音，每个都有自己的密钥，第一次调用前会请你确认；默认关闭。

这是一个**完全原生的程序**：没有内嵌 Python、没有 torch、没有 CUDA wheel。整个工作室是 **Rust + 原生 C++/CUDA 引擎（GGUF/ONNX）**，运行在一个进程里。模型、引擎、CUDA 与 VC++ 运行库和 ffmpeg 在首次运行时**由程序自己下载**；作者其他工作室里已有的模型会直接拿来用，不再重复下载。**本程序为 NVIDIA 显卡构建并测试**；其他情况见“各阶段在哪里运行”。

## 给 AI 智能体

Speech Studio 打开时，会在 `http://127.0.0.1:8797/mcp` 提供 MCP：Claude Code、Claude Desktop、Cursor、Codex 等智能体可以用同一套代码完成窗口能做的一切——一次调用把文本配成文件、逐块创建和编辑项目、给角色分配声音、按描述设计声音、识别并核对录音、导出带章节和字幕的有声书。设置里的 **智能体 (MCP)** 显示是否已连接，以及要粘贴到客户端的内容。

1. 从[最新发布](https://github.com/timoncool/speech-studio/releases/latest)安装并启动工作室。
2. 连接它的 MCP 服务器：
   ```bash
   claude mcp add --transport http speech-studio http://127.0.0.1:8797/mcp
   ```
   其他客户端：`{ "mcpServers": { "speech-studio": { "type": "streamable-http", "url": "http://127.0.0.1:8797/mcp" } } }`
3. 阅读服务器提供的技能说明（资源 `studio://skill`，与 [docs/mcp-skill.md](docs/mcp-skill.md) 内容相同），从工具 `studio_status` 开始。

同一端口上还提供 OpenAI 音频 API——`POST /v1/audio/speech`、`POST /v1/audio/transcriptions`、`GET /v1/models`——为 OpenAI 编写的工具可直接通过它配音和识别，`voice` 填声音库中的声音名称。[llms.txt](llms.txt) 为寻找它的工具提供同样的信息。只有本机上的程序可以连接。

## 两种模式

| 模式 | 作用 |
|------|--------------|
| 🔊 **Speech** | 文本、对话、播客或书 → 带多个版本的配音块，逐句听审，导出为 WAV、FLAC、MP3、OGG、M4A 或带章节的 M4B，附字幕和同步文本 |
| 👂 **Listening** | 音频、视频或链接 → 带说话人和逐词时间的文字稿，卡拉 OK 播放器，按文字剪辑，导出 SRT、VTT、ASS、TXT 或 JSON，与参考文本核对 |

## 功能

**声音与引擎**

- **五个本地引擎**——[Higgs Audio v3](https://huggingface.co/bosonai)（克隆、情绪和声音效果）、**Fish Audio Drama 3**（最强的开源声音；24 GB 显卡可整体加载，6 GB 起专家放在内存中）、**Supertonic 3**（快速，十个内置声音）、**Qwen3-TTS 1.7B**（九个内置声音、克隆、每块一条指令）以及**带俄语重音的 Qwen3-TTS**（运行时加载 siriusfreak 的 LoRA）。
- **角色和块可以有自己的引擎**，覆盖项目引擎；不能说当前声音的引擎会告诉你它会改用哪个内置声音。
- **声音库**——一键导入声音包、云端目录中的 743 个俄语声音、麦克风录音、导入文件和 ZIP、照片、性别、年龄、语言、标签和授权说明；参考音频的文本只识别一次并保存。
- **清理参考音频**——Mel-Band Roformer 把人声从音乐中分离，**DPDFNet** 去除噪声并可调节强度，让语音保持自然；原始文件会保留。
- **参考音频的最佳片段**——按识别出的词之间的停顿切窗，每个窗口给出长度、信噪比、削波和最长停顿。
- **按描述生成声音**——Qwen3-TTS VoiceDesign 根据“一位声音柔和、略带沙哑的老奶奶……”生成声音：试听多个候选，把最好的保存到声音库。

**听起来正确的文本**

- **俄语规范化**——数字按正确的格和性、日期、时间、货币、单位、罗马数字、拉丁字母品牌名和缩写。
- **RUAccent 处理重音、«ё» 和同形异音词**——大号同形词模型（int8 的 big_poetry，选择与 fp32 一致）根据句子判断；有疑问的会高亮供你选择，你自己的 `+` 不会被改动。
- **发音词典**——替换、正则和音素，作用范围可为项目、语言、声音或引擎，可导入 Balabolka 的 `.rex`/`.dic`/`.bxd`，并能在词典里直接试听规则。
- **兼容 SpeechKit 的标记**——`+` 重音、`sil<[300]>` 和 `[pause 500ms]` 停顿、`[rate:1.2]` 语速、`**重读**`；引擎不理解的内容会被标出，而不是悄悄丢掉。

**项目**

- **文本即时间轴**——带说话人的块、多个版本（最多五个）、过期追踪、撤销重做、拖动排序、支持正则的查找替换、注释和标记。
- **质量控制**——每个版本都会被反向识别并逐词比对，另有削波、静音、截断、结尾咔哒声、重复短语、长尾音、音乐或噪声代替语音、与参考声音相似度等检查；多版本自动选出最佳。
- **对话和播客**——`Anna:` 和 `[Anna]` 行会变成角色，导演为 N 个说话人按主题写播客，负停顿形成插话，**时间轴**显示每处重叠，可选择截断、接续或保留。
- **有声书**——EPUB、FB2、PDF、DOCX、HTML、Markdown 和 TXT，章节预览、导演按角色分配、带章节和封面的 M4B、长书分卷、ACX 响度检查。
- **字幕即项目**——SRT、VTT 和 ASS 的台词配到各自的时间段：预先提高语速，不合适的版本会重试，然后再加速并借用台词前的静音。
- **导出前检查**——导出前列出缺音频、过期、失败、文件被改动或比台词更长的块；只有时长不符可以放行。
- **响度**——视频（−14 LUFS）、播客（−16 LUFS）和 ACX 预设，带 true-peak 限幅器、房间底噪和块之间的呼吸声。
- **项目文件**——`.speechproj` 包含文本、声音和音频，可把项目带到另一台电脑。

**Listening**

- **Parakeet-TDT 或 Whisper**，通过 Nemotron 3 Diarization 区分最多八个说话人，长录音由 **Silero VAD v6** 按停顿切分，DPDFNet 去噪，Roformer 去音乐。
- **给 Whisper 的提示**——主题和术语写法，加上你添加的更正。
- **批量**——一次拖入多个文件，每个都会成为排队识别的录音。
- **把已知文本铺到录音上**——参考文本逐句获得每个被听到的词的时间。
- **从说话人生成声音**——用每个说话人最清晰的词生成声音库声音。

**其他**

- **兼容 OpenAI 的 API**、带窗口控制的 **MCP**、使用同一套工具的 OpenRouter 模型助手面板。
- **诊断**——显卡、驱动、CUDA、库、日志，以及不含密钥的问题报告包。
- **整个程序支持六种语言**，浅色和深色主题，自动更新，完全便携。

## 截图

开始界面——文本、书、字幕或播客脚本都能变成项目：

![Speech Studio 开始界面](docs/shots/home-en.png)

Listening——带说话人和逐词时间的文字稿、卡拉 OK 播放器、按文字剪辑和导出：

![Speech Studio Listening](docs/shots/listening-en.png)

## 系统要求

- **系统：** Windows 10 / 11（x64）；Linux x86-64 为实验性构建
- **显卡：** 8 GB 或以上显存的 NVIDIA 显卡和较新的驱动；Drama 3 可整体放入 24 GB；6 GB 起其专家（18 GB）放在内存中，此时需要 32 GB 内存，占用多少显存在设置中选择（“自动”或 5 至 24 GB）。没有 NVIDIA 时只能运行“各阶段在哪里运行”中的 CPU 和云端路径，这种配置未经测试
- **WebView2**——Windows 11 已预装；Windows 10 上安装程序会自动获取
- **磁盘：** 默认模型、引擎和运行库约 9 GB，另加你添加的内容（Drama 3 为 23 GB）

在 NVIDIA 电脑上，唯一需要手动安装的是较新的 **[NVIDIA 驱动](https://www.nvidia.com/Download/index.aspx)**。

## 快速开始

1. 从 [Releases](https://github.com/timoncool/speech-studio/releases) **下载**便携版并解压到任意位置（或用 `-setup.exe` / `.msi` 安装）。
2. **运行** `Speech Studio.exe`。
3. 在**首次运行**面板点击**下载所选**——程序一次性下载模型、引擎和运行库。
4. **粘贴文本**并点击**创建**，或切换到 **Listening** 拖入录音。

> 所有内容都下载并保存在**程序文件夹内**：模型、缓存、声音和项目不会写到别处。

变更记录见 [CHANGELOG.md](CHANGELOG.md)，程序顶部的星光按钮也会显示。贡献者和编程智能体的规则：[AGENTS.md](AGENTS.md)。

## 程序会下载的全部内容

首次运行面板一键下载以下全部内容。在代理之后或无法访问 Hugging Face 时，请在“设置 → **网络**”中设置代理。你也可以自己下载直链文件，按最后一列放到程序文件夹下的对应位置，然后点击**从文件夹导入**；压缩包类组件由程序自行下载。每个文件在使用前都会核对固定的 SHA-256。大小和哈希来自程序清单 `crates/speech-server/src/setup.rs`，本表与其保持一致。

<!-- downloads:start -->
| 组件 | 必要性 | 文件（直接链接） | 大小 | 存放位置 |
|---|---|---|---|---|
| Higgs Audio v3 Q8_0 | 必需 | [q8_0.gguf](https://huggingface.co/drbaph/Higgs-Audio-v3-Studio/resolve/c6e9db5a2062c15accc1b9bfa54d927bbdb124dc/models/higgs-q8_0/q8_0.gguf) → `models\higgs-q8_0\q8_0.gguf`<br>[config.json](https://huggingface.co/drbaph/Higgs-Audio-v3-Studio/resolve/c6e9db5a2062c15accc1b9bfa54d927bbdb124dc/models/higgs-q8_0/config.json) → `models\higgs-q8_0\config.json`<br>[chat_template.jinja](https://huggingface.co/drbaph/Higgs-Audio-v3-Studio/resolve/c6e9db5a2062c15accc1b9bfa54d927bbdb124dc/models/higgs-q8_0/chat_template.jinja) → `models\higgs-q8_0\chat_template.jinja`<br>[tokenizer.json](https://huggingface.co/drbaph/Higgs-Audio-v3-Studio/resolve/c6e9db5a2062c15accc1b9bfa54d927bbdb124dc/models/higgs-q8_0/tokenizer.json) → `models\higgs-q8_0\tokenizer.json`<br>[tokenizer_config.json](https://huggingface.co/drbaph/Higgs-Audio-v3-Studio/resolve/c6e9db5a2062c15accc1b9bfa54d927bbdb124dc/models/higgs-q8_0/tokenizer_config.json) → `models\higgs-q8_0\tokenizer_config.json`<br>[higgs_audio_v2_tokenizer_config.json](https://huggingface.co/drbaph/Higgs-Audio-v3-Studio/resolve/c6e9db5a2062c15accc1b9bfa54d927bbdb124dc/models/higgs-q8_0/higgs_audio_v2_tokenizer_config.json) → `models\higgs-q8_0\higgs_audio_v2_tokenizer_config.json` | 5.5 GB | 原样放入，路径见箭头后 |
| audiocpp_engine.dll (Higgs engine) | 必需 | [audiocpp_engine.dll](https://github.com/timoncool/Higgs-Ultimate/releases/download/engine-0.2.3-turing/audiocpp_engine.dll) → `models\higgs-engine\audiocpp_engine.dll` | 129 MB | 原样放入，路径见箭头后 |
| Gemma-4 12B QAT q4_0 + vision | 可选 | [gemma-4-12b-it-qat-q4_0.gguf](https://huggingface.co/google/gemma-4-12b-it-qat-q4_0-gguf/resolve/2b318d6ebebf093f50ca4376e858325f10703358/gemma-4-12b-it-qat-q4_0.gguf) → `models\mt\gemma-4-12b-it-qat-q4_0.gguf`<br>[mmproj-gemma-4-12b-it-qat-q4_0.gguf](https://huggingface.co/google/gemma-4-12b-it-qat-q4_0-gguf/resolve/2b318d6ebebf093f50ca4376e858325f10703358/mmproj-gemma-4-12b-it-qat-q4_0.gguf) → `models\mt\mmproj-gemma-4-12b-it-qat-q4_0.gguf` | 7.2 GB | 原样放入，路径见箭头后 |
| Gemma-4 12B Q5_K_M + vision | 可选 | [gemma-4-12b-it-Q5_K_M.gguf](https://huggingface.co/unsloth/gemma-4-12b-it-GGUF/resolve/d997c805aafe035a8024f961c6e1afd6b30d79a5/gemma-4-12b-it-Q5_K_M.gguf) → `models\mt-q5_0\gemma-4-12b-it-Q5_K_M.gguf`<br>[mmproj-F16.gguf](https://huggingface.co/unsloth/gemma-4-12b-it-GGUF/resolve/d997c805aafe035a8024f961c6e1afd6b30d79a5/mmproj-F16.gguf) → `models\mt-q5_0\mmproj-F16.gguf` | 8.6 GB | 原样放入，路径见箭头后 |
| Gemma-4 12B Q6_K + vision | 可选 | [gemma-4-12b-it-Q6_K.gguf](https://huggingface.co/unsloth/gemma-4-12b-it-GGUF/resolve/d997c805aafe035a8024f961c6e1afd6b30d79a5/gemma-4-12b-it-Q6_K.gguf) → `models\mt-q6_k\gemma-4-12b-it-Q6_K.gguf`<br>[mmproj-F16.gguf](https://huggingface.co/unsloth/gemma-4-12b-it-GGUF/resolve/d997c805aafe035a8024f961c6e1afd6b30d79a5/mmproj-F16.gguf) → `models\mt-q6_k\mmproj-F16.gguf` | 10.0 GB | 原样放入，路径见箭头后 |
| Gemma-4 12B Q8_0 + vision | 可选 | [gemma-4-12b-it-Q8_0.gguf](https://huggingface.co/unsloth/gemma-4-12b-it-GGUF/resolve/d997c805aafe035a8024f961c6e1afd6b30d79a5/gemma-4-12b-it-Q8_0.gguf) → `models\mt-q8_0\gemma-4-12b-it-Q8_0.gguf`<br>[mmproj-F16.gguf](https://huggingface.co/unsloth/gemma-4-12b-it-GGUF/resolve/d997c805aafe035a8024f961c6e1afd6b30d79a5/mmproj-F16.gguf) → `models\mt-q8_0\mmproj-F16.gguf` | 12.8 GB | 原样放入，路径见箭头后 |
| Parakeet-TDT 0.6B v3 int8 | 必需 | [encoder-model.int8.onnx](https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx/resolve/8f23f0c03c8761650bdb5b40aaf3e40d2c15f1ce/encoder-model.int8.onnx) → `models\tdt\encoder-model.int8.onnx`<br>[decoder_joint-model.int8.onnx](https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx/resolve/8f23f0c03c8761650bdb5b40aaf3e40d2c15f1ce/decoder_joint-model.int8.onnx) → `models\tdt\decoder_joint-model.int8.onnx`<br>[nemo128.onnx](https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx/resolve/8f23f0c03c8761650bdb5b40aaf3e40d2c15f1ce/nemo128.onnx) → `models\tdt\nemo128.onnx`<br>[vocab.txt](https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx/resolve/8f23f0c03c8761650bdb5b40aaf3e40d2c15f1ce/vocab.txt) → `models\tdt\vocab.txt`<br>[config.json](https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx/resolve/8f23f0c03c8761650bdb5b40aaf3e40d2c15f1ce/config.json) → `models\tdt\config.json` | 671 MB | 原样放入，路径见箭头后 |
| Higgs Audio v3 Q6_K | 可选 | [q6_k.gguf](https://huggingface.co/drbaph/Higgs-Audio-v3-Studio/resolve/c6e9db5a2062c15accc1b9bfa54d927bbdb124dc/models/higgs-q6_k/q6_k.gguf) → `models\higgs-q6_k\q6_k.gguf`<br>[config.json](https://huggingface.co/drbaph/Higgs-Audio-v3-Studio/resolve/c6e9db5a2062c15accc1b9bfa54d927bbdb124dc/models/higgs-q6_k/config.json) → `models\higgs-q6_k\config.json`<br>[chat_template.jinja](https://huggingface.co/drbaph/Higgs-Audio-v3-Studio/resolve/c6e9db5a2062c15accc1b9bfa54d927bbdb124dc/models/higgs-q6_k/chat_template.jinja) → `models\higgs-q6_k\chat_template.jinja`<br>[tokenizer.json](https://huggingface.co/drbaph/Higgs-Audio-v3-Studio/resolve/c6e9db5a2062c15accc1b9bfa54d927bbdb124dc/models/higgs-q6_k/tokenizer.json) → `models\higgs-q6_k\tokenizer.json`<br>[tokenizer_config.json](https://huggingface.co/drbaph/Higgs-Audio-v3-Studio/resolve/c6e9db5a2062c15accc1b9bfa54d927bbdb124dc/models/higgs-q6_k/tokenizer_config.json) → `models\higgs-q6_k\tokenizer_config.json`<br>[higgs_audio_v2_tokenizer_config.json](https://huggingface.co/drbaph/Higgs-Audio-v3-Studio/resolve/c6e9db5a2062c15accc1b9bfa54d927bbdb124dc/models/higgs-q6_k/higgs_audio_v2_tokenizer_config.json) → `models\higgs-q6_k\higgs_audio_v2_tokenizer_config.json` | 5.0 GB | 原样放入，路径见箭头后 |
| Higgs Audio v3 Q4_K_M | 可选 | [q4_k_m.gguf](https://huggingface.co/drbaph/Higgs-Audio-v3-Studio/resolve/c6e9db5a2062c15accc1b9bfa54d927bbdb124dc/models/higgs-q4_k_m/q4_k_m.gguf) → `models\higgs-q4_k_m\q4_k_m.gguf`<br>[config.json](https://huggingface.co/drbaph/Higgs-Audio-v3-Studio/resolve/c6e9db5a2062c15accc1b9bfa54d927bbdb124dc/models/higgs-q4_k_m/config.json) → `models\higgs-q4_k_m\config.json`<br>[chat_template.jinja](https://huggingface.co/drbaph/Higgs-Audio-v3-Studio/resolve/c6e9db5a2062c15accc1b9bfa54d927bbdb124dc/models/higgs-q4_k_m/chat_template.jinja) → `models\higgs-q4_k_m\chat_template.jinja`<br>[tokenizer.json](https://huggingface.co/drbaph/Higgs-Audio-v3-Studio/resolve/c6e9db5a2062c15accc1b9bfa54d927bbdb124dc/models/higgs-q4_k_m/tokenizer.json) → `models\higgs-q4_k_m\tokenizer.json`<br>[tokenizer_config.json](https://huggingface.co/drbaph/Higgs-Audio-v3-Studio/resolve/c6e9db5a2062c15accc1b9bfa54d927bbdb124dc/models/higgs-q4_k_m/tokenizer_config.json) → `models\higgs-q4_k_m\tokenizer_config.json`<br>[higgs_audio_v2_tokenizer_config.json](https://huggingface.co/drbaph/Higgs-Audio-v3-Studio/resolve/c6e9db5a2062c15accc1b9bfa54d927bbdb124dc/models/higgs-q4_k_m/higgs_audio_v2_tokenizer_config.json) → `models\higgs-q4_k_m\higgs_audio_v2_tokenizer_config.json` | 4.1 GB | 原样放入，路径见箭头后 |
| Fish Audio Drama 3 Q4 + drama_engine (6 GB+ GPU, 32 GB RAM under 24 GB) | 可选 | [.gitattributes](https://huggingface.co/apiplant/drama-q4/resolve/1fce9bd21d46a0311294aebfad19f994df90902f/.gitattributes) → `models\drama3\.gitattributes`<br>[LICENSE](https://huggingface.co/apiplant/drama-q4/resolve/1fce9bd21d46a0311294aebfad19f994df90902f/LICENSE) → `models\drama3\LICENSE`<br>[NOTICE](https://huggingface.co/apiplant/drama-q4/resolve/1fce9bd21d46a0311294aebfad19f994df90902f/NOTICE) → `models\drama3\NOTICE`<br>[README.md](https://huggingface.co/apiplant/drama-q4/resolve/1fce9bd21d46a0311294aebfad19f994df90902f/README.md) → `models\drama3\README.md`<br>[codec.safetensors](https://huggingface.co/apiplant/drama-q4/resolve/1fce9bd21d46a0311294aebfad19f994df90902f/codec.safetensors) → `models\drama3\codec.safetensors`<br>[drama.json](https://huggingface.co/apiplant/drama-q4/resolve/1fce9bd21d46a0311294aebfad19f994df90902f/drama.json) → `models\drama3\drama.json`<br>[globals.safetensors](https://huggingface.co/apiplant/drama-q4/resolve/1fce9bd21d46a0311294aebfad19f994df90902f/globals.safetensors) → `models\drama3\globals.safetensors`<br>[layer00.safetensors](https://huggingface.co/apiplant/drama-q4/resolve/1fce9bd21d46a0311294aebfad19f994df90902f/layer00.safetensors) → `models\drama3\layer00.safetensors`<br>[layer01.safetensors](https://huggingface.co/apiplant/drama-q4/resolve/1fce9bd21d46a0311294aebfad19f994df90902f/layer01.safetensors) → `models\drama3\layer01.safetensors`<br>[layer02.safetensors](https://huggingface.co/apiplant/drama-q4/resolve/1fce9bd21d46a0311294aebfad19f994df90902f/layer02.safetensors) → `models\drama3\layer02.safetensors`<br>[layer03.safetensors](https://huggingface.co/apiplant/drama-q4/resolve/1fce9bd21d46a0311294aebfad19f994df90902f/layer03.safetensors) → `models\drama3\layer03.safetensors`<br>[layer04.safetensors](https://huggingface.co/apiplant/drama-q4/resolve/1fce9bd21d46a0311294aebfad19f994df90902f/layer04.safetensors) → `models\drama3\layer04.safetensors`<br>[layer05.safetensors](https://huggingface.co/apiplant/drama-q4/resolve/1fce9bd21d46a0311294aebfad19f994df90902f/layer05.safetensors) → `models\drama3\layer05.safetensors`<br>[layer06.safetensors](https://huggingface.co/apiplant/drama-q4/resolve/1fce9bd21d46a0311294aebfad19f994df90902f/layer06.safetensors) → `models\drama3\layer06.safetensors`<br>[layer07.safetensors](https://huggingface.co/apiplant/drama-q4/resolve/1fce9bd21d46a0311294aebfad19f994df90902f/layer07.safetensors) → `models\drama3\layer07.safetensors`<br>[layer08.safetensors](https://huggingface.co/apiplant/drama-q4/resolve/1fce9bd21d46a0311294aebfad19f994df90902f/layer08.safetensors) → `models\drama3\layer08.safetensors`<br>[layer09.safetensors](https://huggingface.co/apiplant/drama-q4/resolve/1fce9bd21d46a0311294aebfad19f994df90902f/layer09.safetensors) → `models\drama3\layer09.safetensors`<br>[layer10.safetensors](https://huggingface.co/apiplant/drama-q4/resolve/1fce9bd21d46a0311294aebfad19f994df90902f/layer10.safetensors) → `models\drama3\layer10.safetensors`<br>[layer11.safetensors](https://huggingface.co/apiplant/drama-q4/resolve/1fce9bd21d46a0311294aebfad19f994df90902f/layer11.safetensors) → `models\drama3\layer11.safetensors`<br>[layer12.safetensors](https://huggingface.co/apiplant/drama-q4/resolve/1fce9bd21d46a0311294aebfad19f994df90902f/layer12.safetensors) → `models\drama3\layer12.safetensors`<br>[layer13.safetensors](https://huggingface.co/apiplant/drama-q4/resolve/1fce9bd21d46a0311294aebfad19f994df90902f/layer13.safetensors) → `models\drama3\layer13.safetensors`<br>[layer14.safetensors](https://huggingface.co/apiplant/drama-q4/resolve/1fce9bd21d46a0311294aebfad19f994df90902f/layer14.safetensors) → `models\drama3\layer14.safetensors`<br>[layer15.safetensors](https://huggingface.co/apiplant/drama-q4/resolve/1fce9bd21d46a0311294aebfad19f994df90902f/layer15.safetensors) → `models\drama3\layer15.safetensors`<br>[layer16.safetensors](https://huggingface.co/apiplant/drama-q4/resolve/1fce9bd21d46a0311294aebfad19f994df90902f/layer16.safetensors) → `models\drama3\layer16.safetensors`<br>[layer17.safetensors](https://huggingface.co/apiplant/drama-q4/resolve/1fce9bd21d46a0311294aebfad19f994df90902f/layer17.safetensors) → `models\drama3\layer17.safetensors`<br>[layer18.safetensors](https://huggingface.co/apiplant/drama-q4/resolve/1fce9bd21d46a0311294aebfad19f994df90902f/layer18.safetensors) → `models\drama3\layer18.safetensors`<br>[layer19.safetensors](https://huggingface.co/apiplant/drama-q4/resolve/1fce9bd21d46a0311294aebfad19f994df90902f/layer19.safetensors) → `models\drama3\layer19.safetensors`<br>[layer20.safetensors](https://huggingface.co/apiplant/drama-q4/resolve/1fce9bd21d46a0311294aebfad19f994df90902f/layer20.safetensors) → `models\drama3\layer20.safetensors`<br>[layer21.safetensors](https://huggingface.co/apiplant/drama-q4/resolve/1fce9bd21d46a0311294aebfad19f994df90902f/layer21.safetensors) → `models\drama3\layer21.safetensors`<br>[layer22.safetensors](https://huggingface.co/apiplant/drama-q4/resolve/1fce9bd21d46a0311294aebfad19f994df90902f/layer22.safetensors) → `models\drama3\layer22.safetensors`<br>[layer23.safetensors](https://huggingface.co/apiplant/drama-q4/resolve/1fce9bd21d46a0311294aebfad19f994df90902f/layer23.safetensors) → `models\drama3\layer23.safetensors`<br>[layer24.safetensors](https://huggingface.co/apiplant/drama-q4/resolve/1fce9bd21d46a0311294aebfad19f994df90902f/layer24.safetensors) → `models\drama3\layer24.safetensors`<br>[layer25.safetensors](https://huggingface.co/apiplant/drama-q4/resolve/1fce9bd21d46a0311294aebfad19f994df90902f/layer25.safetensors) → `models\drama3\layer25.safetensors`<br>[layer26.safetensors](https://huggingface.co/apiplant/drama-q4/resolve/1fce9bd21d46a0311294aebfad19f994df90902f/layer26.safetensors) → `models\drama3\layer26.safetensors`<br>[layer27.safetensors](https://huggingface.co/apiplant/drama-q4/resolve/1fce9bd21d46a0311294aebfad19f994df90902f/layer27.safetensors) → `models\drama3\layer27.safetensors`<br>[layer28.safetensors](https://huggingface.co/apiplant/drama-q4/resolve/1fce9bd21d46a0311294aebfad19f994df90902f/layer28.safetensors) → `models\drama3\layer28.safetensors`<br>[layer29.safetensors](https://huggingface.co/apiplant/drama-q4/resolve/1fce9bd21d46a0311294aebfad19f994df90902f/layer29.safetensors) → `models\drama3\layer29.safetensors`<br>[layer30.safetensors](https://huggingface.co/apiplant/drama-q4/resolve/1fce9bd21d46a0311294aebfad19f994df90902f/layer30.safetensors) → `models\drama3\layer30.safetensors`<br>[layer31.safetensors](https://huggingface.co/apiplant/drama-q4/resolve/1fce9bd21d46a0311294aebfad19f994df90902f/layer31.safetensors) → `models\drama3\layer31.safetensors`<br>[layer32.safetensors](https://huggingface.co/apiplant/drama-q4/resolve/1fce9bd21d46a0311294aebfad19f994df90902f/layer32.safetensors) → `models\drama3\layer32.safetensors`<br>[layer33.safetensors](https://huggingface.co/apiplant/drama-q4/resolve/1fce9bd21d46a0311294aebfad19f994df90902f/layer33.safetensors) → `models\drama3\layer33.safetensors`<br>[layer34.safetensors](https://huggingface.co/apiplant/drama-q4/resolve/1fce9bd21d46a0311294aebfad19f994df90902f/layer34.safetensors) → `models\drama3\layer34.safetensors`<br>[layer35.safetensors](https://huggingface.co/apiplant/drama-q4/resolve/1fce9bd21d46a0311294aebfad19f994df90902f/layer35.safetensors) → `models\drama3\layer35.safetensors`<br>[layer36.safetensors](https://huggingface.co/apiplant/drama-q4/resolve/1fce9bd21d46a0311294aebfad19f994df90902f/layer36.safetensors) → `models\drama3\layer36.safetensors`<br>[layer37.safetensors](https://huggingface.co/apiplant/drama-q4/resolve/1fce9bd21d46a0311294aebfad19f994df90902f/layer37.safetensors) → `models\drama3\layer37.safetensors`<br>[layer38.safetensors](https://huggingface.co/apiplant/drama-q4/resolve/1fce9bd21d46a0311294aebfad19f994df90902f/layer38.safetensors) → `models\drama3\layer38.safetensors`<br>[layer39.safetensors](https://huggingface.co/apiplant/drama-q4/resolve/1fce9bd21d46a0311294aebfad19f994df90902f/layer39.safetensors) → `models\drama3\layer39.safetensors`<br>[tokenizer.json](https://huggingface.co/apiplant/drama-q4/resolve/1fce9bd21d46a0311294aebfad19f994df90902f/tokenizer.json) → `models\drama3\tokenizer.json`<br>[tower.safetensors](https://huggingface.co/apiplant/drama-q4/resolve/1fce9bd21d46a0311294aebfad19f994df90902f/tower.safetensors) → `models\drama3\tower.safetensors`<br>[drama_engine.dll](https://github.com/timoncool/drama-rs/releases/download/engine-0.1.3/drama_engine.dll) → `models\drama-engine\drama_engine.dll` | 23.0 GB | 原样放入，路径见箭头后 |
| Supertonic 3 int8 | 可选 | [LICENSE](https://huggingface.co/askurios8/supertonic-3-int8/resolve/95618f5d61cef4923c3b802a332fd603723718f7/LICENSE) → `models\supertonic3\LICENSE`<br>[duration_predictor.onnx](https://huggingface.co/askurios8/supertonic-3-int8/resolve/95618f5d61cef4923c3b802a332fd603723718f7/onnx/duration_predictor.onnx) → `models\supertonic3\onnx\duration_predictor.onnx`<br>[text_encoder.onnx](https://huggingface.co/askurios8/supertonic-3-int8/resolve/95618f5d61cef4923c3b802a332fd603723718f7/onnx/text_encoder.onnx) → `models\supertonic3\onnx\text_encoder.onnx`<br>[vector_estimator.onnx](https://huggingface.co/askurios8/supertonic-3-int8/resolve/95618f5d61cef4923c3b802a332fd603723718f7/onnx/vector_estimator.onnx) → `models\supertonic3\onnx\vector_estimator.onnx`<br>[vocoder.onnx](https://huggingface.co/askurios8/supertonic-3-int8/resolve/95618f5d61cef4923c3b802a332fd603723718f7/onnx/vocoder.onnx) → `models\supertonic3\onnx\vocoder.onnx`<br>[tts.json](https://huggingface.co/askurios8/supertonic-3-int8/resolve/95618f5d61cef4923c3b802a332fd603723718f7/onnx/tts.json) → `models\supertonic3\onnx\tts.json`<br>[unicode_indexer.json](https://huggingface.co/askurios8/supertonic-3-int8/resolve/95618f5d61cef4923c3b802a332fd603723718f7/onnx/unicode_indexer.json) → `models\supertonic3\onnx\unicode_indexer.json`<br>[F1.json](https://huggingface.co/askurios8/supertonic-3-int8/resolve/95618f5d61cef4923c3b802a332fd603723718f7/voice_styles/F1.json) → `models\supertonic3\voice_styles\F1.json`<br>[F2.json](https://huggingface.co/askurios8/supertonic-3-int8/resolve/95618f5d61cef4923c3b802a332fd603723718f7/voice_styles/F2.json) → `models\supertonic3\voice_styles\F2.json`<br>[F3.json](https://huggingface.co/askurios8/supertonic-3-int8/resolve/95618f5d61cef4923c3b802a332fd603723718f7/voice_styles/F3.json) → `models\supertonic3\voice_styles\F3.json`<br>[F4.json](https://huggingface.co/askurios8/supertonic-3-int8/resolve/95618f5d61cef4923c3b802a332fd603723718f7/voice_styles/F4.json) → `models\supertonic3\voice_styles\F4.json`<br>[F5.json](https://huggingface.co/askurios8/supertonic-3-int8/resolve/95618f5d61cef4923c3b802a332fd603723718f7/voice_styles/F5.json) → `models\supertonic3\voice_styles\F5.json`<br>[M1.json](https://huggingface.co/askurios8/supertonic-3-int8/resolve/95618f5d61cef4923c3b802a332fd603723718f7/voice_styles/M1.json) → `models\supertonic3\voice_styles\M1.json`<br>[M2.json](https://huggingface.co/askurios8/supertonic-3-int8/resolve/95618f5d61cef4923c3b802a332fd603723718f7/voice_styles/M2.json) → `models\supertonic3\voice_styles\M2.json`<br>[M3.json](https://huggingface.co/askurios8/supertonic-3-int8/resolve/95618f5d61cef4923c3b802a332fd603723718f7/voice_styles/M3.json) → `models\supertonic3\voice_styles\M3.json`<br>[M4.json](https://huggingface.co/askurios8/supertonic-3-int8/resolve/95618f5d61cef4923c3b802a332fd603723718f7/voice_styles/M4.json) → `models\supertonic3\voice_styles\M4.json`<br>[M5.json](https://huggingface.co/askurios8/supertonic-3-int8/resolve/95618f5d61cef4923c3b802a332fd603723718f7/voice_styles/M5.json) → `models\supertonic3\voice_styles\M5.json` | 108 MB | 原样放入，路径见箭头后 |
| Qwen3-TTS engine (qwen.dll) and codec | 可选 | [qwen-tokenizer-12hz-Q8_0.gguf](https://huggingface.co/Serveurperso/Qwen3-TTS-GGUF/resolve/b7ee2e8c7459c3bea99da23e3d178125a7d1713c/qwen-tokenizer-12hz-Q8_0.gguf) → `models\qwen3-tts\qwen-tokenizer-12hz-Q8_0.gguf`<br>[qwen.dll](https://github.com/timoncool/qwentts.cpp/releases/download/engine-0.1.0/qwen.dll) → `models\qwen3-tts\qwen.dll`<br>[ggml.dll](https://github.com/timoncool/qwentts.cpp/releases/download/engine-0.1.0/ggml.dll) → `models\qwen3-tts\ggml.dll`<br>[ggml-base.dll](https://github.com/timoncool/qwentts.cpp/releases/download/engine-0.1.0/ggml-base.dll) → `models\qwen3-tts\ggml-base.dll`<br>[ggml-cpu.dll](https://github.com/timoncool/qwentts.cpp/releases/download/engine-0.1.0/ggml-cpu.dll) → `models\qwen3-tts\ggml-cpu.dll`<br>[ggml-cuda.dll](https://github.com/timoncool/qwentts.cpp/releases/download/engine-0.1.0/ggml-cuda.dll) → `models\qwen3-tts\ggml-cuda.dll` | 434 MB | 原样放入，路径见箭头后 |
| Qwen3-TTS 1.7B Q4_K_M (CustomVoice + Base) | 可选 | [qwen-talker-1.7b-customvoice-Q4_K_M.gguf](https://huggingface.co/Serveurperso/Qwen3-TTS-GGUF/resolve/b7ee2e8c7459c3bea99da23e3d178125a7d1713c/qwen-talker-1.7b-customvoice-Q4_K_M.gguf) → `models\qwen3-tts\qwen-talker-1.7b-customvoice-Q4_K_M.gguf`<br>[qwen-talker-1.7b-base-Q4_K_M.gguf](https://huggingface.co/Serveurperso/Qwen3-TTS-GGUF/resolve/b7ee2e8c7459c3bea99da23e3d178125a7d1713c/qwen-talker-1.7b-base-Q4_K_M.gguf) → `models\qwen3-tts\qwen-talker-1.7b-base-Q4_K_M.gguf` | 2.4 GB | 原样放入，路径见箭头后 |
| Qwen3-TTS 1.7B Q8_0 (CustomVoice + Base) | 可选 | [qwen-talker-1.7b-customvoice-Q8_0.gguf](https://huggingface.co/Serveurperso/Qwen3-TTS-GGUF/resolve/b7ee2e8c7459c3bea99da23e3d178125a7d1713c/qwen-talker-1.7b-customvoice-Q8_0.gguf) → `models\qwen3-tts\qwen-talker-1.7b-customvoice-Q8_0.gguf`<br>[qwen-talker-1.7b-base-Q8_0.gguf](https://huggingface.co/Serveurperso/Qwen3-TTS-GGUF/resolve/b7ee2e8c7459c3bea99da23e3d178125a7d1713c/qwen-talker-1.7b-base-Q8_0.gguf) → `models\qwen3-tts\qwen-talker-1.7b-base-Q8_0.gguf` | 4.1 GB | 原样放入，路径见箭头后 |
| Qwen3-TTS VoiceDesign Q4_K_M | 可选 | [qwen-talker-1.7b-voicedesign-Q4_K_M.gguf](https://huggingface.co/Serveurperso/Qwen3-TTS-GGUF/resolve/b7ee2e8c7459c3bea99da23e3d178125a7d1713c/qwen-talker-1.7b-voicedesign-Q4_K_M.gguf) → `models\qwen3-tts\qwen-talker-1.7b-voicedesign-Q4_K_M.gguf` | 1.2 GB | 原样放入，路径见箭头后 |
| Qwen3-TTS VoiceDesign Q8_0 | 可选 | [qwen-talker-1.7b-voicedesign-Q8_0.gguf](https://huggingface.co/Serveurperso/Qwen3-TTS-GGUF/resolve/b7ee2e8c7459c3bea99da23e3d178125a7d1713c/qwen-talker-1.7b-voicedesign-Q8_0.gguf) → `models\qwen3-tts\qwen-talker-1.7b-voicedesign-Q8_0.gguf` | 2.0 GB | 原样放入，路径见箭头后 |
| Qwen3-TTS Russian stress LoRA | 可选 | [adapter.safetensors](https://huggingface.co/siriusfreak/qwen3-tts-12hz-1.7b-ru-stress-cf/resolve/c16b3af3db3095120efc1ebb73f6522442bcb3b1/lora/adapter.safetensors) → `models\qwen3-tts\qwen-talker-1.7b-base-ru-stress-lora.safetensors` | 35 MB | 原样放入，路径见箭头后 |
| Parakeet Ultra 0.6B int8 (Moondream) | 可选 | [encoder-model.int8.onnx](https://huggingface.co/Masterx/parakeet-tdt-0.6b-ultra-onnx/resolve/99b09f030a5a6efeaa13cf2cf54592100ce2c3f1/encoder-model.int8.onnx) → `models\tdt-ultra-int8\encoder-model.int8.onnx`<br>[decoder_joint-model.int8.onnx](https://huggingface.co/Masterx/parakeet-tdt-0.6b-ultra-onnx/resolve/99b09f030a5a6efeaa13cf2cf54592100ce2c3f1/decoder_joint-model.int8.onnx) → `models\tdt-ultra-int8\decoder_joint-model.int8.onnx`<br>[nemo128.onnx](https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx/resolve/8f23f0c03c8761650bdb5b40aaf3e40d2c15f1ce/nemo128.onnx) → `models\tdt-ultra-int8\nemo128.onnx`<br>[vocab.txt](https://huggingface.co/Masterx/parakeet-tdt-0.6b-ultra-onnx/resolve/99b09f030a5a6efeaa13cf2cf54592100ce2c3f1/vocab.txt) → `models\tdt-ultra-int8\vocab.txt`<br>[config.json](https://huggingface.co/Masterx/parakeet-tdt-0.6b-ultra-onnx/resolve/99b09f030a5a6efeaa13cf2cf54592100ce2c3f1/config.json) → `models\tdt-ultra-int8\config.json` | 671 MB | 原样放入，路径见箭头后 |
| Nemotron 3.5 ASR 0.6B int8 (NVIDIA) | 可选 | [encoder.onnx](https://huggingface.co/smcleod/nemotron-3.5-asr-streaming-0.6b-int8/resolve/f1f26d22dab5c4eabe6d01b63c906889e7e817d3/encoder.onnx) → `models\nemotron-asr-int8\encoder.onnx`<br>[encoder.onnx.data](https://huggingface.co/smcleod/nemotron-3.5-asr-streaming-0.6b-int8/resolve/f1f26d22dab5c4eabe6d01b63c906889e7e817d3/encoder.onnx.data) → `models\nemotron-asr-int8\encoder.onnx.data`<br>[decoder_joint.onnx](https://huggingface.co/smcleod/nemotron-3.5-asr-streaming-0.6b-int8/resolve/f1f26d22dab5c4eabe6d01b63c906889e7e817d3/decoder_joint.onnx) → `models\nemotron-asr-int8\decoder_joint.onnx`<br>[tokenizer.model](https://huggingface.co/smcleod/nemotron-3.5-asr-streaming-0.6b-int8/resolve/f1f26d22dab5c4eabe6d01b63c906889e7e817d3/tokenizer.model) → `models\nemotron-asr-int8\tokenizer.model` | 683 MB | 原样放入，路径见箭头后 |
| Whisper-Faster (faster-whisper standalone) | 可选 | [Whisper-Faster_r192.3_windows.zip](https://github.com/Purfview/whisper-standalone-win/releases/download/faster-whisper/Whisper-Faster_r192.3_windows.zip) | 88 MB | 解压文件（不含子文件夹）到 `tools\whisper\` |
| Whisper CUDA (cuBLAS 11, cuDNN 8) | 可选 | [libcublas-windows-x86_64-11.11.3.6-archive.zip](https://developer.download.nvidia.com/compute/cuda/redist/libcublas/windows-x86_64/libcublas-windows-x86_64-11.11.3.6-archive.zip)<br>[cudnn-windows-x86_64-8.9.7.29_cuda11-archive.zip](https://developer.download.nvidia.com/compute/cudnn/redist/cudnn/windows-x86_64/cudnn-windows-x86_64-8.9.7.29_cuda11-archive.zip) | 1.1 GB | 取出压缩包中所有 .dll 放入 `tools\whisper\` |
| Whisper tiny | 可选 | [model.bin](https://huggingface.co/Systran/faster-whisper-tiny/resolve/d90ca5fe260221311c53c58e660288d3deb8d356/model.bin) → `models\whisper\faster-whisper-tiny\model.bin`<br>[config.json](https://huggingface.co/Systran/faster-whisper-tiny/resolve/d90ca5fe260221311c53c58e660288d3deb8d356/config.json) → `models\whisper\faster-whisper-tiny\config.json`<br>[tokenizer.json](https://huggingface.co/Systran/faster-whisper-tiny/resolve/d90ca5fe260221311c53c58e660288d3deb8d356/tokenizer.json) → `models\whisper\faster-whisper-tiny\tokenizer.json`<br>[vocabulary.txt](https://huggingface.co/Systran/faster-whisper-tiny/resolve/d90ca5fe260221311c53c58e660288d3deb8d356/vocabulary.txt) → `models\whisper\faster-whisper-tiny\vocabulary.txt` | 78 MB | 原样放入，路径见箭头后 |
| Whisper base | 可选 | [model.bin](https://huggingface.co/Systran/faster-whisper-base/resolve/ebe41f70d5b6dfa9166e2c581c45c9c0cfc57b66/model.bin) → `models\whisper\faster-whisper-base\model.bin`<br>[config.json](https://huggingface.co/Systran/faster-whisper-base/resolve/ebe41f70d5b6dfa9166e2c581c45c9c0cfc57b66/config.json) → `models\whisper\faster-whisper-base\config.json`<br>[tokenizer.json](https://huggingface.co/Systran/faster-whisper-base/resolve/ebe41f70d5b6dfa9166e2c581c45c9c0cfc57b66/tokenizer.json) → `models\whisper\faster-whisper-base\tokenizer.json`<br>[vocabulary.txt](https://huggingface.co/Systran/faster-whisper-base/resolve/ebe41f70d5b6dfa9166e2c581c45c9c0cfc57b66/vocabulary.txt) → `models\whisper\faster-whisper-base\vocabulary.txt` | 148 MB | 原样放入，路径见箭头后 |
| Whisper small | 可选 | [model.bin](https://huggingface.co/Systran/faster-whisper-small/resolve/536b0662742c02347bc0e980a01041f333bce120/model.bin) → `models\whisper\faster-whisper-small\model.bin`<br>[config.json](https://huggingface.co/Systran/faster-whisper-small/resolve/536b0662742c02347bc0e980a01041f333bce120/config.json) → `models\whisper\faster-whisper-small\config.json`<br>[tokenizer.json](https://huggingface.co/Systran/faster-whisper-small/resolve/536b0662742c02347bc0e980a01041f333bce120/tokenizer.json) → `models\whisper\faster-whisper-small\tokenizer.json`<br>[vocabulary.txt](https://huggingface.co/Systran/faster-whisper-small/resolve/536b0662742c02347bc0e980a01041f333bce120/vocabulary.txt) → `models\whisper\faster-whisper-small\vocabulary.txt` | 486 MB | 原样放入，路径见箭头后 |
| Whisper medium | 可选 | [model.bin](https://huggingface.co/Systran/faster-whisper-medium/resolve/08e178d48790749d25932bbc082711ddcfdfbc4f/model.bin) → `models\whisper\faster-whisper-medium\model.bin`<br>[config.json](https://huggingface.co/Systran/faster-whisper-medium/resolve/08e178d48790749d25932bbc082711ddcfdfbc4f/config.json) → `models\whisper\faster-whisper-medium\config.json`<br>[tokenizer.json](https://huggingface.co/Systran/faster-whisper-medium/resolve/08e178d48790749d25932bbc082711ddcfdfbc4f/tokenizer.json) → `models\whisper\faster-whisper-medium\tokenizer.json`<br>[vocabulary.txt](https://huggingface.co/Systran/faster-whisper-medium/resolve/08e178d48790749d25932bbc082711ddcfdfbc4f/vocabulary.txt) → `models\whisper\faster-whisper-medium\vocabulary.txt` | 1.5 GB | 原样放入，路径见箭头后 |
| Whisper large-v3 | 可选 | [model.bin](https://huggingface.co/Systran/faster-whisper-large-v3/resolve/edaa852ec7e145841d8ffdb056a99866b5f0a478/model.bin) → `models\whisper\faster-whisper-large-v3\model.bin`<br>[config.json](https://huggingface.co/Systran/faster-whisper-large-v3/resolve/edaa852ec7e145841d8ffdb056a99866b5f0a478/config.json) → `models\whisper\faster-whisper-large-v3\config.json`<br>[preprocessor_config.json](https://huggingface.co/Systran/faster-whisper-large-v3/resolve/edaa852ec7e145841d8ffdb056a99866b5f0a478/preprocessor_config.json) → `models\whisper\faster-whisper-large-v3\preprocessor_config.json`<br>[tokenizer.json](https://huggingface.co/Systran/faster-whisper-large-v3/resolve/edaa852ec7e145841d8ffdb056a99866b5f0a478/tokenizer.json) → `models\whisper\faster-whisper-large-v3\tokenizer.json`<br>[vocabulary.json](https://huggingface.co/Systran/faster-whisper-large-v3/resolve/edaa852ec7e145841d8ffdb056a99866b5f0a478/vocabulary.json) → `models\whisper\faster-whisper-large-v3\vocabulary.json` | 3.1 GB | 原样放入，路径见箭头后 |
| Whisper large-v3-turbo | 可选 | [model.bin](https://huggingface.co/deepdml/faster-whisper-large-v3-turbo-ct2/resolve/4df90f75321148c3a29a9e2351b7ddf8f5b115a8/model.bin) → `models\whisper\faster-whisper-large-v3-turbo\model.bin`<br>[config.json](https://huggingface.co/deepdml/faster-whisper-large-v3-turbo-ct2/resolve/4df90f75321148c3a29a9e2351b7ddf8f5b115a8/config.json) → `models\whisper\faster-whisper-large-v3-turbo\config.json`<br>[preprocessor_config.json](https://huggingface.co/deepdml/faster-whisper-large-v3-turbo-ct2/resolve/4df90f75321148c3a29a9e2351b7ddf8f5b115a8/preprocessor_config.json) → `models\whisper\faster-whisper-large-v3-turbo\preprocessor_config.json`<br>[tokenizer.json](https://huggingface.co/deepdml/faster-whisper-large-v3-turbo-ct2/resolve/4df90f75321148c3a29a9e2351b7ddf8f5b115a8/tokenizer.json) → `models\whisper\faster-whisper-large-v3-turbo\tokenizer.json`<br>[vocabulary.json](https://huggingface.co/deepdml/faster-whisper-large-v3-turbo-ct2/resolve/4df90f75321148c3a29a9e2351b7ddf8f5b115a8/vocabulary.json) → `models\whisper\faster-whisper-large-v3-turbo\vocabulary.json` | 1.6 GB | 原样放入，路径见箭头后 |
| Nemotron 3 Diarization | 推荐 | [nemotron3_diar_v3.onnx](https://huggingface.co/altunenes/parakeet-rs/resolve/4d2a8bc71f5c896ec40faa59732e6716295edaf2/nemotron-3-diarization/nemotron3_diar_v3.onnx) → `models\nemotron-diar\nemotron3_diar_v3.onnx`<br>[LICENSE](https://huggingface.co/altunenes/parakeet-rs/resolve/4d2a8bc71f5c896ec40faa59732e6716295edaf2/nemotron-3-diarization/LICENSE) → `models\nemotron-diar\LICENSE` | 401 MB | 原样放入，路径见箭头后 |
| Mel-Band Roformer voc_fv6 Q8_0 | 推荐 | [voc_fv6-Q8_0.gguf](https://huggingface.co/chenmozhijin/BSRoformer-GGUF/resolve/df802a6773d25ba6ef785ff619daa3e510503168/GaboxR67/MelBandRoformers/melbandroformers/vocals/voc_fv6-Q8_0.gguf) → `models\bsroformer\voc_fv6-Q8_0.gguf` | 252 MB | 原样放入，路径见箭头后 |
| Mel-Band Roformer voc_fv6 Q5_0 | 可选 | [voc_fv6-Q5_0.gguf](https://huggingface.co/chenmozhijin/BSRoformer-GGUF/resolve/df802a6773d25ba6ef785ff619daa3e510503168/GaboxR67/MelBandRoformers/melbandroformers/vocals/voc_fv6-Q5_0.gguf) → `models\bsroformer\voc_fv6-Q5_0.gguf` | 167 MB | 原样放入，路径见箭头后 |
| Mel-Band Roformer voc_fv6 Q4_0 | 可选 | [voc_fv6-Q4_0.gguf](https://huggingface.co/chenmozhijin/BSRoformer-GGUF/resolve/df802a6773d25ba6ef785ff619daa3e510503168/GaboxR67/MelBandRoformers/melbandroformers/vocals/voc_fv6-Q4_0.gguf) → `models\bsroformer\voc_fv6-Q4_0.gguf` | 139 MB | 原样放入，路径见箭头后 |
| WeSpeaker ResNet34-LM (voice similarity) | 推荐 | [voxceleb_resnet34_LM.onnx](https://huggingface.co/Wespeaker/wespeaker-voxceleb-resnet34-LM/resolve/f0c48c298fd835726c27956a5d617bad7115627e/voxceleb_resnet34_LM.onnx) → `models\faces\wespeaker\voxceleb_resnet34_LM.onnx` | 27 MB | 原样放入，路径见箭头后 |
| RUAccent (stress, «ё», homographs) | 推荐 | [model.onnx](https://huggingface.co/ruaccent/accentuator/resolve/b78ae5ea1e62beaf138bed1865cd8c3b0b5ca855/nn/nn_accent/model.onnx) → `models\ruaccent\nn\nn_accent\model.onnx`<br>[config.json](https://huggingface.co/ruaccent/accentuator/resolve/b78ae5ea1e62beaf138bed1865cd8c3b0b5ca855/nn/nn_accent/config.json) → `models\ruaccent\nn\nn_accent\config.json`<br>[vocab.txt](https://huggingface.co/ruaccent/accentuator/resolve/b78ae5ea1e62beaf138bed1865cd8c3b0b5ca855/nn/nn_accent/vocab.txt) → `models\ruaccent\nn\nn_accent\vocab.txt`<br>[model.onnx](https://huggingface.co/ruaccent/accentuator/resolve/b78ae5ea1e62beaf138bed1865cd8c3b0b5ca855/nn/nn_omograph/tiny2/model.onnx) → `models\ruaccent\nn\nn_omograph\tiny2\model.onnx`<br>[tokenizer.json](https://huggingface.co/ruaccent/accentuator/resolve/b78ae5ea1e62beaf138bed1865cd8c3b0b5ca855/nn/nn_omograph/tiny2/tokenizer.json) → `models\ruaccent\nn\nn_omograph\tiny2\tokenizer.json`<br>[accents.json.gz](https://huggingface.co/ruaccent/accentuator/resolve/b78ae5ea1e62beaf138bed1865cd8c3b0b5ca855/dictionary/accents.json.gz) → `models\ruaccent\dictionary\accents.json.gz`<br>[omographs.json.gz](https://huggingface.co/ruaccent/accentuator/resolve/b78ae5ea1e62beaf138bed1865cd8c3b0b5ca855/dictionary/omographs.json.gz) → `models\ruaccent\dictionary\omographs.json.gz`<br>[yo_words.json.gz](https://huggingface.co/ruaccent/accentuator/resolve/b78ae5ea1e62beaf138bed1865cd8c3b0b5ca855/dictionary/yo_words.json.gz) → `models\ruaccent\dictionary\yo_words.json.gz`<br>[yo_homographs.json.gz](https://huggingface.co/ruaccent/accentuator/resolve/b78ae5ea1e62beaf138bed1865cd8c3b0b5ca855/dictionary/yo_homographs.json.gz) → `models\ruaccent\dictionary\yo_homographs.json.gz` | 34 MB | 原样放入，路径见箭头后 |
| RUAccent big_poetry homographs int8 | 可选 | [model.onnx](https://huggingface.co/nerualdreming/ruaccent-omograph-int8/resolve/d5c4eb6864d55cfc24ae45530eb4ef96843efb2b/big_poetry/model.onnx) → `models\ruaccent\nn\nn_omograph\big_poetry\model.onnx`<br>[tokenizer.json](https://huggingface.co/nerualdreming/ruaccent-omograph-int8/resolve/d5c4eb6864d55cfc24ae45530eb4ef96843efb2b/big_poetry/tokenizer.json) → `models\ruaccent\nn\nn_omograph\big_poetry\tokenizer.json` | 185 MB | 原样放入，路径见箭头后 |
| Silero VAD v6 | 推荐 | [silero_vad.onnx](https://huggingface.co/istupakov/silero-vad-onnx/resolve/b3e3ee3cce4c11ceb63b1a0b229d916069c1ddf6/silero_vad.onnx) → `models\silero-vad\silero_vad.onnx` | 2 MB | 原样放入，路径见箭头后 |
| DPDFNet 48 kHz (noise suppression) | 推荐 | [dpdfnet2_48khz_hr.onnx](https://huggingface.co/Ceva-IP/DPDFNet/resolve/dd6818d00f50c836fed43a6243ebe49116de5964/onnx/dpdfnet2_48khz_hr.onnx) → `models\dpdfnet\dpdfnet2_48khz_hr.onnx` | 10 MB | 原样放入，路径见箭头后 |
| BSRoformer.cpp (CUDA) | 推荐 | [BSRoformer-windows-cuda-13.1.0.zip](https://github.com/chenmozhijin/BSRoformer.cpp/releases/download/v0.1.0/BSRoformer-windows-cuda-13.1.0.zip) | 165 MB | 解压文件（不含子文件夹）到 `tools\bsroformer\` |
| BSRoformer.cpp (CPU) | 可选 | [BSRoformer-windows-x64-msvc.zip](https://github.com/chenmozhijin/BSRoformer.cpp/releases/download/v0.1.0/BSRoformer-windows-x64-msvc.zip) | 671 KB | 解压文件（不含子文件夹）到 `tools\bsroformer-cpu\` |
| llama.cpp server (CUDA) | 可选 | [llama-b11146-bin-win-cuda-13.4-x64.zip](https://github.com/ggml-org/llama.cpp/releases/download/b11146/llama-b11146-bin-win-cuda-13.4-x64.zip) | 150 MB | 解压文件（不含子文件夹）到 `tools\llama\` |
| ONNX Runtime | 必需 | [onnxruntime-win-x64-1.28.2.zip](https://github.com/microsoft/onnxruntime/releases/download/v1.28.2/onnxruntime-win-x64-1.28.2.zip) | 79 MB | 保留文件夹结构解压到 `models\runtime\` |
| ONNX Runtime GPU (CUDA) | 推荐 | [onnxruntime-win-x64-gpu_cuda13-1.28.2.zip](https://github.com/microsoft/onnxruntime/releases/download/v1.28.2/onnxruntime-win-x64-gpu_cuda13-1.28.2.zip) | 366 MB | 保留文件夹结构解压到 `models\runtime\` |
| FFmpeg (static build) | 必需 | [ffmpeg-N-126342-gf88b741dbf-win64-gpl.zip](https://github.com/BtbN/FFmpeg-Builds/releases/download/autobuild-2026-08-31-13-27/ffmpeg-N-126342-gf88b741dbf-win64-gpl.zip) | 171 MB | 从压缩包中取出 ffmpeg.exe 和 ffprobe.exe 放入 `tools\ffmpeg\` |
| yt-dlp + deno | 可选 | [yt-dlp.exe](https://github.com/yt-dlp/yt-dlp/releases/download/2026.08.19/yt-dlp.exe) → `tools\yt-dlp\yt-dlp.exe`<br>[deno-x86_64-pc-windows-msvc.zip](https://github.com/denoland/deno/releases/download/v2.9.7/deno-x86_64-pc-windows-msvc.zip) | 60 MB | 原样放入，路径见箭头后<br>解压文件（不含子文件夹）到 `tools\yt-dlp\` |
| CUDA runtime (cudart, cuBLAS, cuFFT) | 必需 | [nvidia_cuda_runtime-13.4.92-py3-none-win_amd64.whl](https://files.pythonhosted.org/packages/86/00/d5436004268f049214193659ebc36550b5ef3925c3d13b4cc980e13be6f5/nvidia_cuda_runtime-13.4.92-py3-none-win_amd64.whl)<br>[nvidia_cublas-13.8.0.4-py3-none-win_amd64.whl](https://files.pythonhosted.org/packages/a3/df/f1246959833e2c437db8be3e5b477f66b87f8817821ed40de6c7561c9a36/nvidia_cublas-13.8.0.4-py3-none-win_amd64.whl)<br>[libcufft-windows-x86_64-12.4.0.43-archive.zip](https://developer.download.nvidia.com/compute/cuda/redist/libcufft/windows-x86_64/libcufft-windows-x86_64-12.4.0.43-archive.zip) | 586 MB | 取出压缩包中所有 .dll 放入 `models\higgs-engine\` |
| cuDNN 9 | 推荐 | [nvidia_cudnn_cu13-9.27.0.42-py3-none-win_amd64.whl](https://files.pythonhosted.org/packages/87/6a/e55ff0ac26a5c6e2b21f41c9d04ad096b4ed6da593fba7e25845c61b0532/nvidia_cudnn_cu13-9.27.0.42-py3-none-win_amd64.whl) | 436 MB | 取出压缩包中所有 .dll 放入 `models\higgs-engine\` |
<!-- downloads:end -->

## 各阶段在哪里运行

| 阶段 | 引擎 | NVIDIA 显卡 | CPU | 云端 |
|---|---|---|---|---|
| 声音与克隆 | Higgs Audio v3、Drama 3、Qwen3-TTS | 是（CUDA） | 否 | OpenRouter、Gemini、fal.ai、Runware |
| 声音 | Supertonic 3 | 是（ONNX Runtime CUDA） | 是 | — |
| 语音识别 | Parakeet-TDT 或 Whisper | 是 | 是 | OpenRouter、fal.ai |
| 说话人 | Nemotron 3 Diarization | 是 | 是 | 否 |
| 停顿与噪声 | Silero VAD v6、DPDFNet | 否 | 是 | 否 |
| 从音乐中分离人声 | Mel-Band Roformer（BSRoformer.cpp） | 是 | 是（单独构建） | 否 |
| 重音与同形异音词 | RUAccent | 否 | 是 | 否 |
| 导演（播客、角色） | Gemma-4 12B（llama.cpp） | 是 | 否 | OpenRouter |

## 故障排除

**安装程序卡在 WebView2。** 使用微软的 [Evergreen Standalone x64](https://go.microsoft.com/fwlink/p/?LinkId=2124701) 离线安装程序安装 WebView2，然后重新运行 Speech Studio 安装程序。

**下载停滞或失败。** 大文件会从中断处继续，再点一次按钮即可。若无法访问 Hugging Face 或 GitHub，请在“设置 → **网络**”中设置代理，或把表中的文件放到位后点击**从文件夹导入**。

**声音引擎提示显卡或驱动过旧。** 本地引擎针对 CUDA 13 和 Turing 及更新的显卡（GTX 16、RTX 20 及以上）构建。请更新 NVIDIA 驱动；旧显卡请使用云端声音。

**识别找不到语音。** 录音比语音阈值更安静：在“识别 → 按停顿切分”中降低阈值。

**设置 → 诊断**可生成不含密钥的问题报告包，请附在 [issue](https://github.com/timoncool/speech-studio/issues) 中。

## 工作原理

**技术栈：** 原生 **Tauri 2（Rust）** 外壳在同一进程中运行 `speech-server`（axum，`127.0.0.1:8797`），并打开 SPA 窗口——React 19 + Vite + Tailwind。引擎通过各自的 C ABI 运行在进程内：Higgs（`audiocpp_engine.dll`）、Drama 3（`drama_engine.dll`，[timoncool/drama-rs](https://github.com/timoncool/drama-rs)）、Qwen3-TTS（`qwen.dll`，[timoncool/qwentts.cpp](https://github.com/timoncool/qwentts.cpp)），Supertonic、Parakeet、Nemotron、Silero VAD、DPDFNet 和 RUAccent 运行在 ONNX Runtime 上，Whisper 和 BSRoformer.cpp 作为辅助程序，ffmpeg 处理音频。**运行时没有任何 Python 进程。**

### 从源码构建

```bash
git clone https://github.com/timoncool/speech-studio.git
cd speech-studio

cd frontend && npm install && npm run build && cd ..   # 1) SPA
cargo build --release -p speech-server                  # 2) 原生服务器（axum）
cd desktop && npm install && npx tauri build            # 3) 桌面外壳（Tauri）
```

需要 Node 20+、Rust（MSVC 工具链）和 WebView2。引擎无需重新构建——程序会下载预编译的二进制文件。

### Linux（实验性）

Linux x86-64 的 .deb 和 AppImage 为**实验性**：同一份代码，配上同样引擎的 Linux 构建，作者没有在真实 Linux 桌面上运行过。非常欢迎以 pull request 提交修复。NVIDIA 驱动（580 或更新）、`libgomp1` 和 `libssl3` 来自系统；模型和引擎存放在 `~/.local/share/speech-studio`。

## 匿名统计与新闻

程序启动时及之后每小时向作者的服务器获取新闻，请求不带任何标识。首次运行界面有一个默认勾选的**发送匿名使用统计**复选框；设置中也有同样的开关，旁边是**发送了什么**。勾选时，程序发送当天的报告：本机生成的随机安装 id、程序版本、操作系统、界面语言、显卡厂商、显存档位以及 CUDA 是否可用、完成/失败/取消的任务数量、各阶段使用的模型，以及任务失败原因（一行文字，已去除路径、名称和引号中的文本）。绝不发送：文本、录音、声音、文件名或任何个人信息。环境变量 `DO_NOT_TRACK=1` 或 `STUDIO_TELEMETRY=0` 会完全关闭统计。

## 更多便携 AI 程序

| 项目 | 简介 |
|--------|----------|
| [Dub Studio](https://github.com/timoncool/dub-studio) | AI 视频配音：克隆声音、翻译字幕和画面文字 |
| [Higgs Ultimate](https://github.com/timoncool/Higgs-Ultimate) | 原生语音合成与声音克隆（Higgs Audio v3） |
| [ACE-Step Studio](https://github.com/timoncool/ACE-Step-Studio) | AI 音乐工作室——歌曲、人声、翻唱、MV |
| [YuE2 Studio](https://github.com/timoncool/YuE2-Studio) | 带可编辑乐谱的 AI 歌曲生成器 |
| [MiniMax Music3 Studio](https://github.com/timoncool/MiniMax-Music3-Studio) | 基于 MiniMax Music 3 的本地/云端原生音乐工作室 |
| [VideoSOS](https://github.com/timoncool/videosos) | AI 视频工作室 |

## 参与与分支

非常欢迎协作者：移植到 **AMD / Intel 显卡、macOS 或 Linux**，修复 Linux 构建，增加界面语言（`frontend/src/locales/`、`crates/speech-server/locales/`）——fork 后提交 pull request。

## 作者

- **Nerual Dreming** — [Telegram](https://t.me/nerual_dreming) | [neuro-cartel.com](https://neuro-cartel.com) | [ArtGeneration.me](https://artgeneration.me) 创始人
- **Neuro-Soft** — [Telegram](https://t.me/neuroport) | 便携 AI 程序

## 致谢

- **[Boson AI](https://huggingface.co/bosonai)**——Higgs Audio v3；**[drbaph / Higgs-Audio-v3-Studio](https://huggingface.co/drbaph/Higgs-Audio-v3-Studio)**——其 GGUF 量化和原生引擎。
- **[Fish Audio](https://fish.audio)**——Drama 3；**[apiplant/drama-q4](https://huggingface.co/apiplant/drama-q4)**——Q4 权重；**[drama-rs](https://github.com/timoncool/drama-rs)**——Rust/CUDA 引擎。
- **[Qwen 团队](https://huggingface.co/Qwen)**——Qwen3-TTS；**[ServeurpersoCom / qwentts.cpp](https://github.com/ServeurpersoCom/qwentts.cpp)** 和 **[Serveurperso/Qwen3-TTS-GGUF](https://huggingface.co/Serveurperso/Qwen3-TTS-GGUF)**——C++ 引擎和 GGUF 量化；**[siriusfreak](https://huggingface.co/siriusfreak/qwen3-tts-12hz-1.7b-ru-stress-cf)**——俄语重音 LoRA。
- **[Supertone](https://huggingface.co/Supertone)**——Supertonic 3；**[askurios8/supertonic-3-int8](https://huggingface.co/askurios8/supertonic-3-int8)**——int8 导出。
- **[RUAccent](https://huggingface.co/ruaccent)**——重音、«ё» 和同形异音词。
- **[NVIDIA](https://huggingface.co/nvidia)**——Parakeet-TDT 和 Nemotron 3 Diarization；ONNX 导出来自 [istupakov](https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx) 和 [altunenes/parakeet-rs](https://github.com/altunenes/parakeet-rs)。
- **[Silero](https://github.com/snakers4/silero-vad)**——Silero VAD v6（[istupakov/silero-vad-onnx](https://huggingface.co/istupakov/silero-vad-onnx)）；**[Ceva-IP / DPDFNet](https://huggingface.co/Ceva-IP/DPDFNet)**——降噪。
- **[Systran / faster-whisper](https://github.com/SYSTRAN/faster-whisper)** 和 **[Purfview](https://github.com/Purfview/whisper-standalone-win)**——Whisper 及其独立构建。
- **[chenmozhijin / BSRoformer.cpp](https://github.com/chenmozhijin/BSRoformer.cpp)** 和 **[GaboxR67](https://huggingface.co/GaboxR67)**——人声分离。
- **[WeSpeaker](https://huggingface.co/Wespeaker/wespeaker-voxceleb-resnet34-LM)**——声音相似度；**[Google Gemma](https://huggingface.co/google)** 和 **[llama.cpp](https://github.com/ggml-org/llama.cpp)**——导演。
- **[ONNX Runtime](https://github.com/microsoft/onnxruntime)**、**[FFmpeg](https://ffmpeg.org)**、**[Tauri](https://tauri.app)**、**[ort](https://github.com/pykeio/ort)** 以及 NVIDIA CUDA 库。

## 支持作者

我开发开源软件并从事 AI 研究——我做的大部分东西都免费开放。捐助能让我做更多开发和研究。

**[所有支持方式](DONATE.md)** | **[dalink.to/nerual_dreming](https://dalink.to/nerual_dreming)** | **[boosty.to/neuro_art](https://boosty.to/neuro_art)**

- **BTC:** `1E7dHL22RpyhJGVpcvKdbyZgksSYkYeEBC`
- **ETH (ERC20):** `0xb5db65adf478983186d4897ba92fe2c25c594a0c`
- **USDT (TRC20):** `TQST9Lp2TjK6FiVkn4fwfGUee7NmkxEE7C`

## Star 历史

<a href="https://github.com/timoncool/speech-studio/stargazers">
 <img alt="Star 历史图" src="https://api.star-history.com/svg?repos=timoncool/speech-studio&type=Date" />
</a>

## 许可证

程序代码采用 [MIT](LICENSE)。**模型不是**：每个模型保留自己的许可证。发布或出售配音内容前请先阅读。

| 组件 | 许可证 | 含义 |
|---|---|---|
| Higgs Audio v3（Boson AI） | [Boson Higgs TTS 3 Research and Non-Commercial License](https://huggingface.co/bosonai/higgs-tts-3-4b/blob/main/LICENSE) | 研究和个人使用；数字创作者在注明 Boson AI 的 Higgs Audio 时可依据 Creator Use Grant 将自己的内容变现；为他人提供产品或服务需要 Boson 的许可 |
| Fish Audio Drama 3 | [Fish Audio Community License](https://huggingface.co/fishaudio/drama-3/blob/main/LICENSE) | 研究和非商业使用；年收入低于 50 万美元时可商业使用，超过则需 Fish Audio 许可；不得提供托管的文字转语音服务 |
| Qwen3-TTS（Qwen） | Apache-2.0 |  |
| Qwen3-TTS 俄语重音 LoRA（siriusfreak） | Apache-2.0 |  |
| Supertonic 3（Supertone） | BigScience OpenRAIL-M | 请阅读其使用限制 |
| RUAccent 及其 int8 同形词模型 | MIT |  |
| Parakeet-TDT、Nemotron 3 Diarization（NVIDIA） | CC-BY-4.0、OpenMDW-1.1 | 需注明 NVIDIA |
| Silero VAD | MIT |  |
| DPDFNet（Ceva） | Apache-2.0 |  |
| Whisper 模型和 faster-whisper | MIT |  |
| Mel-Band Roformer voc_fv6、GGUF | 模型页面未注明许可证 | 商业使用前请询问作者 |
| WeSpeaker ResNet34-LM | CC-BY-4.0 | 需注明 WeSpeaker |
| Gemma-4 12B（Google） | Gemma 条款 | 请阅读 |
| llama.cpp、ONNX Runtime、BSRoformer.cpp、qwentts.cpp | MIT |  |
| FFmpeg（BtbN 构建） | GPL 构建 | 作为独立程序，不链接进本程序 |
| NVIDIA CUDA runtime、cuBLAS、cuFFT、cuDNN | NVIDIA 自有条款 | 从 NVIDIA 和 PyPI 下载 |

<sub>文本转语音 · 声音克隆 · 俄语 TTS · 重音标注 · 有声书制作 · 播客 · 语音识别 · 转写 · 说话人分离 · 离线 · 本地 · 开源 · Windows</sub>
