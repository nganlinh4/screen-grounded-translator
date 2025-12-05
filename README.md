# Screen Goated Toolbox (SGT)

A powerful Windows utility that captures any region of your screen **or records system/microphone audio** and processes it using advanced AI models. Whether you need to translate text, extract code (OCR), summarize content, get image descriptions, **or transcribe meetings**, SGT handles it with customizable presets and global hotkeys.

## Key Features

- **Multi-Modal Support:** Utilize **Groq** (Llama 4, Whisper, GPT-OSS) or **Google Gemini** (Flash, Pro) for Vision, Text, and Audio processing.
- **Audio Intelligence:** Record and transcribe/translate audio from your **Microphone** or **System Sound** (Device Audio/Loopback).
- **Preset System:** Create unlimited custom profiles (e.g., "Translate Image", "Transcribe Meeting", "Quick Foreigner Reply").
- **Advanced Hotkeys:** Assign custom key combinations (e.g., `Ctrl+Alt+T`, `Win+Shift+S`) to specific presets.
- **Retranslation Pipeline:** Chain models (e.g., Vision/Audio -> Text Model) for higher accuracy.
- **Smart Overlay:**
  - Streaming text support (Typewriter effect).
  - Auto-copy to clipboard.
  - "Broom" cursor for precise selection.
  - Linked windows for dual-view.
- **Usage Statistics:** Monitor your API usage limits directly in the app.
- **Localization:** UI available in English, Vietnamese, and Korean.

## Screenshot

![Screenshot](docs/images/screenshot.png)
![Demo Video](docs/images/demo-video.gif)

## Prerequisites

- **OS:** Windows 10 or Windows 11.
- **API Keys:**
  - **Groq:** [Get a free key here](https://console.groq.com/keys) (Required for Llama, Whisper, & GPT-OSS models).
  - **Google Gemini:** [Get a free key here](https://aistudio.google.com/app/apikey) (Required for Gemini Vision & Audio models).

## Installation

### Option 1: Download Release
Download the latest `.exe` from the [Releases](https://github.com/nganlinh4/screen-goated-toolbox/releases) page.

### Option 2: Build from Source
Ensure [Rust](https://www.rust-lang.org/tools/install) is installed.

```bash
git clone https://github.com/nganlinh4/screen-goated-toolbox
cd screen-goated-toolbox
# The build script will handle icon resource embedding automatically
cargo build --release
```

Run the executable found in `target/release/`.

## Getting Started

1. **Launch the App:** Open `screen-goated-toolbox.exe`.
2. **Global Settings:**
   - Paste your **Groq API Key** and/or **Gemini API Key**.
   - Toggle **Run at Windows Startup** if desired.
3. **Configure a Preset:**
   - Select a preset on the left or create a new one.
   - **Type:** Choose `Image Understanding` or `Audio Understanding`.
   - **Prompt:** Define the AI instruction (e.g., "Translate to {language1}").
   - **Model:** Select your preferred model (e.g., `Llama 4 Scout`, `Gemini Flash`, `Whisper`).
   - **Hotkeys:** Click "Add Key" to assign a shortcut.
4. **Capture:**
   - **Image:** Press hotkey -> Drag to select area -> Result appears in overlay.
   - **Audio:** Press hotkey -> Recording overlay appears -> Press hotkey again to finish.

## Configuration Guide

### Preset Types
* **Image Understanding:** Captures a screen region (OCR, Translation, Description).
* **Audio Understanding:** Records audio from **Mic** or **Device** (System Audio). Useful for meetings, videos, or quick voice commands.
* **Video Understanding:** (Upcoming feature).

### Retranslation (Pipeline)
For higher quality results, SGT can chain models:
1. **Extraction:** Vision/Audio model extracts raw text/transcript.
2. **Retranslation:** A specialized Text model (e.g., `GPT-OSS`, `Kimi`, `Gemini`) translates/refines the output.

### Available Models

**Vision Models (Image):**
* `Scout` (Llama 4 Scout 17B) - Extremely fast, good for general text.
* `Maverick` (Llama 4 Maverick 17B) - Highly accurate instruction following.
* `Gemini Flash Lite` (Google) - Efficient and fast.
* `Gemini Flash` (Google) - Balanced performance.
* `Gemini 2.5 Pro` (Google) - Highest accuracy, best for reasoning.

**Audio Models (Speech):**
* `Whisper Fast` (Large v3 Turbo) - Fast transcription via Groq.
* `Whisper Accurate` (Large v3) - High accuracy transcription via Groq.
* `Gemini Audio` (Flash Lite / Flash / 2.5 Pro) - Native multimodal audio understanding (can summarize/translate directly).

**Text Models (Retranslation):**
* `Fast Text` (GPT-OSS 20B) - Super fast.
* `Fast 120B` (GPT-OSS 120B) - Balanced speed/quality.
* `Accurate` (Kimi k2-instruct) - High quality Chinese/English handling.
* `Gemini Text` (Flash Lite / Flash / 2.5 Pro) - Google's text capabilities.

## Troubleshooting

**Hotkey conflict / Not working:**
* If using the app in games or elevated applications, **run SGT as Administrator**.
* Check for conflicts with other apps.

**"NO_API_KEY" Error:**
* Ensure keys are entered in "Global Settings".
* Verify the selected preset uses a model matching the provider key you entered (Groq vs Google).

**Audio Recording Issues:**
* Ensure your default microphone or output device is active in Windows Sound Settings.
* If recording "Device Audio", play some sound to ensure the loopback stream has data.

## License

MIT — See [LICENSE](LICENSE) file.

## Credits

Developed by **nganlinh4**.
* Powered by [Groq](https://groq.com) and [Google DeepMind](https://deepmind.google/technologies/gemini/).
* Built with [Rust](https://www.rust-lang.org/) and [egui](https://github.com/emilk/egui).
