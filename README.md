# Screen Goated Toolbox (SGT)

**The Ultimate AI Productivity Automation Tool for Windows.**

Screen Goated Toolbox (SGT) is a native Windows utility that bridges your screen, system audio, and microphone with the world's most powerful AI models. It allows you to create custom AI workflows using a visual node graph to automate tasks like OCR, translation, meeting transcription, generative audio, and text analysis.

## Key Features

### 🧠 Multi-Modal AI Support

* **Cloud Providers:** Native integration with **Groq** (Llama 3, Whisper), **Google Gemini** (Flash, Pro, Gemma), and **OpenRouter** (Claude, GPT-4, DeepSeek).
* **Local AI:** Full support for **Ollama** to run private, local vision and text models without internet.

### ⛓️ Node Graph Workflow

Create complex presets using a visual editor. Connect blocks to define logic:

* **Input:** Screen Region (Snipping), Microphone, System Audio Loopback, Text Selection, or File Drag-and-Drop.
* **Process:** Chain multiple models (e.g., *Speech to Text* -> *Translate* -> *Summarize*).
* **Output:** Streaming Overlay, Markdown View, Text-to-Speech, or Clipboard.

### 🎙️ Audio Intelligence

* **Real-time "Cabin" Mode:** Live, low-latency transcription and translation overlay. Works with **System Audio** (Zoom/Youtube/Games) or **Microphone**.
* **Per-App Capture:** Target audio from specific running applications.
* **PromptDJ:** A dedicated MIDI-controlled interface for generative music and audio control.

### 🛠️ Productivity Tools

* **Smart Overlays:**
  * **Result Overlay:** Interactive window with streaming text, markdown rendering, and "Refine" chat.
  * **Preset Wheel:** A circular menu (`Win+Shift+S` style) to quickly select tools at cursor position.
  * **Favorite Bubble:** A floating dock for instant access to common presets.
* **Text-to-Speech:** High-quality reading using Edge TTS, Gemini Live, or Google Translate.
* **History Gallery:** Auto-saves captures, transcriptions, and generated audio in a searchable database.

## Installation

### Option 1: Download Release

Download the latest `.exe` from the [Releases](https://github.com/nganlinh4/screen-goated-toolbox/releases) page.

* **Standard:** `ScreenGoatedToolbox_v4.0.0.exe` (Compressed, smaller).
* **NoPack:** `..._nopack.exe` (Use this if Windows Defender triggers a false positive).

### Option 2: Build from Source

**Prerequisites:**

* [Rust](https://www.rust-lang.org/) (Nightly toolchain required).
* [Node.js](https://nodejs.org/) (Required for building the PromptDJ frontend).
* **Visual Studio Build Tools 2022** with "Desktop development with C++" workload.

```bash
git clone https://github.com/nganlinh4/screen-goated-toolbox
cd screen-goated-toolbox

# 1. Setup dependencies and patch libraries
powershell -ExecutionPolicy Bypass -File scripts/setup-egui-snarl.ps1

# 2. Build the application (Script handles Frontend build + Rust build + UPX)
powershell -ExecutionPolicy Bypass -File build.ps1
```

The executable will be located in `target/release/`.

## Getting Started

1. **Launch SGT:** Run the executable.
2. **Global Settings:**
    * Click the **Settings** icon in the sidebar.
    * Enter API Keys for the providers you wish to use (Groq, Gemini, OpenRouter).
    * *(Optional)* Enable **Ollama** if you have it installed locally.
3. **Select a Preset:**
    * Use the sidebar to choose a built-in preset (e.g., "Translate Region", "Transcribe Speech").
    * Assign a **Global Hotkey** (e.g., `Alt+Q`) to the preset.
4. **Usage:**
    * Press your hotkey.
    * **For Image Presets:** Drag to select a screen area.
    * **For Audio Presets:** Recording starts automatically (or opens the Realtime overlay).

## Advanced Configuration

### The Node Graph

SGT v4 uses a node-based system for Presets.

1. **Create Preset:** Click `+` in the sidebar.
2. **Input Node:** Choose "Image", "Audio", or "Text".
3. **Process Node:** Select your AI Model and enter a System Prompt (e.g., "Translate this to Vietnamese").
4. **Connect:** Drag wires between nodes to define the data flow.
5. **Variables:** Use `{language1}` in your prompt to allow dynamic language selection via the UI.

### Real-time Translation (Cabin Mode)

1. Select/Create an **Audio** preset.
2. Set **Processing Mode** to **Realtime (Live)**.
3. Set **Source** to **Device** (System Audio) or **Mic**.
4. Launch the preset. A minimalist overlay will appear showing live subtitles.
5. *Tip:* You can toggle Transcription, Translation, and TTS directly from the overlay.

### Using Local AI (Ollama)

1. Install [Ollama](https://ollama.com/).
2. Pull models: `ollama pull llama3` (text) or `ollama pull moondream` (vision).
3. In SGT **Global Settings**, enable Ollama and set the URL (default: `http://localhost:11434`).
4. In your Preset's **Process Node**, select the model from the "Local" section.

## Troubleshooting

**"NO_API_KEY" Error**

* Go to Global Settings and ensure you have pasted a valid key for the model provider selected in your preset (Groq vs Google vs OpenRouter).

**WebView2 / Blank UI**

* SGT uses Microsoft Edge WebView2 for complex rendering (Markdown, Charts, PromptDJ). Ensure the [WebView2 Runtime](https://developer.microsoft.com/en-us/microsoft-edge/webview2/) is installed on your Windows machine.

**Audio Recording is Silent**

* **Device Audio:** Ensure audio is actually playing through your default output device.
* **Permissions:** Check Windows Privacy settings to ensure the app has access to the Microphone.

**PromptDJ / MIDI Not Working**

* Ensure your MIDI controller is connected *before* launching SGT.
* Click "Refresh Devices" inside the PromptDJ interface.

## License

MIT License — See [LICENSE](LICENSE) file.

## Credits

Developed by **nganlinh4**.

* **UI Framework:** [egui](https://github.com/emilk/egui) & [wry](https://github.com/tauri-apps/wry).
* **Audio:** [cpal](https://github.com/RustAudio/cpal) & [symphonia](https://github.com/pdeljanov/Symphonia).
* **AI Providers:** Groq, Google DeepMind, OpenRouter.
