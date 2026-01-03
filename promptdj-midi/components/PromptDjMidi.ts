/**
 * @license
 * SPDX-License-Identifier: Apache-2.0
*/
import { css, html, LitElement } from 'lit';
import { customElement, property, state, query } from 'lit/decorators.js';
import { styleMap } from 'lit/directives/style-map.js';

import { throttle } from '../utils/throttle';

import './PromptController';
import './PlayPauseMorphWrapper';
import type { PlaybackState, Prompt } from '../types';
import { MidiDispatcher } from '../utils/MidiDispatcher';
import { LOCALES, Lang } from '../utils/Locales';

/** The grid of prompt inputs. */
@customElement('prompt-dj-midi')
export class PromptDjMidi extends LitElement {
  static override styles = css`
    :host {
      height: 100%;
      display: flex;
      flex-direction: column;
      justify-content: center;
      align-items: center;
      box-sizing: border-box;
      font-family: 'Google Sans Flex', 'Segoe UI', system-ui, sans-serif;
    }
    button {
      font-family: inherit;
    }
    #background {
      will-change: background-image;
      position: absolute;
      height: 100%;
      width: 100%;
      z-index: -1;
      background: var(--md-surface);
    }
    /* Main layout: grid on the left, controls on the right */
    #content {
      display: flex;
      align-items: center;
      justify-content: center;
      gap: 8vmin;
      position: relative;
    }

    /* Grid wrapper includes just the grid now */
    #gridWrap {
      display: flex;
      align-items: center;
      justify-content: center;
      gap: 2.5vmin;
      height: 80vmin;
    }

    /* #addColumn removed */

    .add-slot {
      width: 17vmin;
      height: 11vmin;
      display: inline-flex;
      align-items: center;
      justify-content: center;
      background: transparent;
      border: none;
      cursor: pointer;
    }
    .add-slot .add-icon {
      width: 9vmin;
      height: 9vmin;
      color: #fff;
      filter: drop-shadow(0 12px 22px rgba(0,0,0,0.25)) drop-shadow(0 4px 10px rgba(0,0,0,0.18));
      transition: transform var(--md-duration-short3) var(--md-easing-emphasized);
    }
    :host([data-theme="light"]) .add-slot .add-icon { color: #fff; }
    .add-slot:hover .add-icon { transform: scale(1.05); }

    #grid {
      width: 120vmin;
      height: 80vmin;
      display: grid;
      grid-template-columns: repeat(6, 1fr);
      gap: 2.5vmin;
    }
    .pc-wrap {
      position: relative;
      overflow: visible;
    }
    .pc-clear {
      position: absolute;
      top: -1.2vmin;
      right: -1.2vmin;
      width: 4.2vmin;
      height: 4.2vmin;
      border-radius: 9999px;
      border: 1px solid var(--md-outline-variant);
      background: var(--md-surface);
      color: var(--md-on-surface);
      display: inline-flex;
      align-items: center;
      justify-content: center;
      padding: 0;
      line-height: 0;
      box-sizing: border-box;
      cursor: pointer;
      box-shadow: var(--md-elevation-level1);
      opacity: 0;
      z-index: 20;
      pointer-events: auto;
      transition: opacity var(--md-duration-short3) var(--md-easing-standard),
                  transform var(--md-duration-short3) var(--md-easing-standard),
                  box-shadow var(--md-duration-short3) var(--md-easing-standard),
                  background-color var(--md-duration-short3) var(--md-easing-standard),
                  border-color var(--md-duration-short3) var(--md-easing-standard);
      transform: scale(0.9);
    }
    .pc-clear svg { width: 100%; height: 100%; display: block; }
    .pc-wrap:hover .pc-clear { opacity: 1; transform: scale(1); }
    .pc-clear:hover {
      background: var(--md-surface-variant);
      border-color: var(--md-outline);
      box-shadow: var(--md-elevation-level2);
      transform: scale(1.06);
    }
    .pc-clear:active {
      transform: scale(0.96);
      box-shadow: var(--md-elevation-level1);
    }
    .pc-clear:focus-visible {
      outline: none;
      box-shadow: 0 0 0 0.22vmin rgba(0,0,0,0.3), var(--md-elevation-level2);
    }

    /* Modal Styling */
    .modal-overlay {
      position: fixed;
      top: 0;
      left: 0;
      width: 100%;
      height: 100%;
      background: rgba(0, 0, 0, 0.6);
      backdrop-filter: blur(5px);
      z-index: 1000;
      display: flex;
      align-items: center;
      justify-content: center;
      animation: fadeIn 0.3s ease;
    }
    .modal-content {
      background: var(--md-surface, #222);
      padding: 3vmin;
      border-radius: 2vmin;
      box-shadow: 0 10px 30px rgba(0,0,0,0.5);
      border: 1px solid rgba(255,255,255,0.1);
      display: flex;
      flex-direction: column;
      gap: 2vmin;
      min-width: 40vmin;
      max-width: 90%;
      transform: translateY(0);
      animation: slideIn 0.3s ease;
    }
    @keyframes fadeIn { from { opacity: 0; } to { opacity: 1; } }
    @keyframes slideIn { from { transform: translateY(20px); opacity: 0; } to { transform: translateY(0); opacity: 1; } }

    .modal-header {
      display: flex;
      justify-content: space-between;
      align-items: center;
      font-size: 2.2vmin;
      font-weight: bold;
      color: var(--md-on-surface, #fff);
      font-family: 'Google Sans Flex', 'Segoe UI', system-ui, sans-serif;
    }
    .close-modal {
      background: transparent;
      border: none;
      color: rgba(255,255,255,0.6);
      cursor: pointer;
      font-size: 3vmin;
      line-height: 1;
      padding: 0;
      font-family: 'Google Sans Flex', 'Segoe UI', system-ui, sans-serif;
    }
    .close-modal:hover { color: #fff; }

    .audio-player {
      width: 100%;
      height: 6vmin;
      border-radius: 999px;
      margin-top: 1vmin;
    }
    
    .download-btn {
      background: var(--md-primary, #6200ea);
      color: var(--md-on-primary, #fff);
      border: none;
      padding: 1.5vmin 3vmin;
      border-radius: 4vmin;
      font-size: 2vmin;
      cursor: pointer;
      display: flex;
      align-items: center;
      justify-content: center;
      gap: 1vmin;
      font-weight: 500;
      font-family: inherit;
      transition: background 0.2s;
    }
    .download-btn:hover {
      filter: brightness(1.2);
    }

    #sideControls {
      display: flex;
      flex-direction: column;
      align-items: center;
      justify-content: center;
      height: 80vmin;
    }
    play-pause-morph {
      width: 23vmin;
      height: 23vmin;
      display: inline-block;
    }


  
    .mini-controls {
      display: flex;
      gap: 1.5vmin;
      margin-top: 4vmin;
    }
    .mini-btn {
      width: 7vmin;
      height: 7vmin;
      background: transparent;
      border: none;
      color: white;
      display: flex;
      align-items: center;
      justify-content: center;
      cursor: pointer;
      transition: all 0.2s ease;
    }
    .mini-btn:hover { transform: scale(1.15); }
    .mini-btn.active { color: #ff3c3c; filter: drop-shadow(0 0 1vmin #ff3c3c); }
    .mini-btn.toggled { color: var(--md-primary); filter: drop-shadow(0 0 1vmin var(--md-primary)); }
    .material-symbols-rounded {
      font-family: 'Material Symbols Rounded';
      font-weight: normal;
      font-style: normal;
      display: inline-block;
      line-height: 1;
      text-transform: none;
      letter-spacing: normal;
      word-wrap: normal;
      white-space: nowrap;
      direction: ltr;
      -webkit-font-smoothing: antialiased;
      font-variation-settings: 'FILL' 1, 'wght' 400, 'grad' 0, 'opsz' 24;
      font-size: 4vmin;
      filter: drop-shadow(0 2px 4px rgba(0,0,0,0.5));
    }
    :host([data-theme="light"]) .material-symbols-rounded {
      filter: drop-shadow(0 1px 2px rgba(0,0,0,0.3));
    }
    .mini-btn .material-symbols-rounded { font-size: 3.5vmin; }
    .pc-clear .material-symbols-rounded { font-size: 2.8vmin; }
    .add-slot .material-symbols-rounded { font-size: 10vmin; }

    .rec-timer-container {
      min-height: 5vmin;
      display: flex;
      flex-direction: column;
      align-items: center;
      justify-content: center;
      font-variant-numeric: tabular-nums;
      margin-top: 1vmin;
      pointer-events: none;
    }
    .rec-timer-elapsed {
      font-size: 1.15em;
      font-weight: 700;
      color: var(--accent-color, #ff4444);
      line-height: 1;
      font-stretch: 125%;
      font-variation-settings: 'wdth' 125;
    }
    .rec-timer-audio {
      font-size: 0.75em; 
      opacity: 0.7;
      margin-top: 2px;
    }

    .volume-container {
      display: flex;
      align-items: center;
      justify-content: center;
      width: 100%;
      margin-bottom: 4vmin;
      box-sizing: border-box;
      gap: 1vmin;
      opacity: 0;
      transition: opacity 0.3s ease;
    }
    #sideControls:hover .volume-container {
      opacity: 1;
    }
    .volume-icon {
      color: var(--md-on-surface);
      font-size: 3vmin;
    }
    .volume-slider {
      flex: 1;
      -webkit-appearance: none;
      height: 0.6vmin;
      border-radius: 1vmin;
      /* Full opacity track to match buttons */
      background: #ffffff;
      outline: none;
      cursor: pointer;
    }
    :host([data-theme="light"]) .volume-slider { background: #444444; }
    .volume-slider::-webkit-slider-thumb {
      -webkit-appearance: none;
      width: 2vmin;
      height: 2vmin;
      border-radius: 50%;
      background: var(--md-primary, #ffffff);
      cursor: pointer;
      transition: transform 0.1s;
      box-shadow: 0 1px 3px rgba(0,0,0,0.3);
    }
    .volume-slider::-webkit-slider-thumb:hover { transform: scale(1.2); }
  `;

  private prompts: Map<string, Prompt>;
  private midiDispatcher: MidiDispatcher;

  @property({ type: Boolean }) private showMidi = false;
  @property({ type: String }) public playbackState: PlaybackState = 'stopped';
  @property({ type: String }) public lang: string = 'en';
  @property({ type: Boolean }) public apiKeySet = false;
  @property({ type: Number }) public audioLevel = 0;
  private lastUserAction: 'play' | 'pause' | null = null;

  @state() private isRecording = false;
  @state() private recordElapsed = 0;
  @state() private recordAudioElapsed = 0;
  private recordInterval: number | null = null;

  // Recording playback state
  @state() private recordingUrl: string | null = null;

  @state() private midiInputIds: string[] = [];
  @state() private activeMidiInputId: string | null = null;
  @state() private optimisticLoading: boolean = false;
  @state() private optimisticPlaying: boolean | null = null; // null = follow real state
  @state() private downloaded = false;
  private clickCooldownUntil: number = 0; // epoch ms; during this window, ignore extra toggles

  // Background drift control
  @state() private driftStrength: number = 0; // 0 = at base, 1 = full drift
  private driftTarget: number = 0;
  private driftRaf: number | null = null;
  private lastDriftTick = 0;

  // Left add-column activation state (4 slots)
  @state() private addSlotsActive: boolean[] = [false, false, false, false];
  // Track which base grid slots are removed (to render add buttons in-grid)
  @state() private removedSlots: Set<string> = new Set();

  @state() private volume: number = 1.0;

  @property({ type: Object })
  private filteredPrompts = new Set<string>();

  private basePrompts: Map<string, Prompt>;
  private baseOrder: string[] = [];
  private readonly STORAGE_KEY = 'pdj_midi_state_v1';

  constructor(
    initialPrompts: Map<string, Prompt>,
  ) {
    super();
    // Deep-copy base prompts
    this.basePrompts = new Map<string, Prompt>();
    for (const [k, p] of initialPrompts.entries()) {
      this.basePrompts.set(k, { ...p });
      this.baseOrder.push(k);
    }
    // Start with defaults, BUT leave the last 2 (Custom 1/2) as empty slots
    // There are 24 slots total. We want 22 active, 2 empty.
    this.prompts = new Map();
    let count = 0;
    for (const [k, p] of this.basePrompts.entries()) {
      if (count < 22) {
        this.prompts.set(k, { ...p });
      }
      count++;
    }
    this.midiDispatcher = new MidiDispatcher();

    // Load saved state and MERGE it
    try {
      const raw = localStorage.getItem(this.STORAGE_KEY);
      if (raw) {
        const parsed = JSON.parse(raw);
        const savedPrompts: any[] = Array.isArray(parsed?.prompts) ? parsed.prompts : [];

        savedPrompts.forEach((p) => {
          if (p && typeof p.promptId === 'string') {
            // Only update if it corresponds to a valid base prompt or a known extra
            // (For now, we trust the defaults structure more for the base grid)
            if (this.prompts.has(p.promptId)) {
              const existing = this.prompts.get(p.promptId)!;
              // Restore USER state (text, weight, color) but keep structure valid
              this.prompts.set(p.promptId, {
                ...existing,
                text: typeof p.text === 'string' ? p.text : existing.text,
                weight: typeof p.weight === 'number' ? p.weight : existing.weight,
                color: p.color || existing.color,
                cc: typeof p.cc === 'number' ? p.cc : existing.cc
              });
            } else if (p.promptId.startsWith('extra-')) {
              // Restore extra slots
              this.prompts.set(p.promptId, {
                promptId: p.promptId,
                text: p.text || '',
                weight: p.weight || 0,
                cc: p.cc || 0,
                color: p.color || '#ffffff'
              });
            }
          }
        });

        if (Array.isArray(parsed?.addSlotsActive) && parsed.addSlotsActive.length === 4) {
          this.addSlotsActive = parsed.addSlotsActive.map((b: any) => !!b);
        }
        const rs = parsed?.removedSlots;
        if (Array.isArray(rs)) {
          // Only respect removal of keys that actually exist in our base order
          const validRemovals = rs.filter((x: any) => typeof x === 'string' && this.basePrompts.has(x));
          this.removedSlots = new Set<string>(validRemovals);
        }
      }
    } catch { }
  }

  public showRecording(blob: Blob) {
    if (this.recordingUrl) {
      URL.revokeObjectURL(this.recordingUrl);
    }
    this.recordingUrl = URL.createObjectURL(blob);
    this.requestUpdate();
  }

  private closeModal() {
    if (this.recordingUrl) {
      URL.revokeObjectURL(this.recordingUrl);
      this.recordingUrl = null;
    }
  }

  private downloadRecording() {
    if (!this.recordingUrl) return;
    const a = document.createElement('a');
    a.href = this.recordingUrl;
    a.download = `PromptDJ_${new Date().toISOString().replace(/:/g, '-')}.wav`;
    a.click();
    this.downloaded = true;
    setTimeout(() => { this.downloaded = false; }, 3000);
  }

  private renderModal() {
    const labels = LOCALES[this.lang as Lang];
    if (!this.recordingUrl) return html``;
    return html`
      <div class="modal-overlay" @click=${this.closeModal}>
        <div class="modal-content" @click=${(e: Event) => e.stopPropagation()}>
          <div class="modal-header">
            <div style="flex: 1; display: flex; flex-direction: column; gap: 2px;">
              <span style="display: block;">${this.downloaded ? labels.saved : labels.recording_ready}</span>
              <div style="font-size: 0.85em; opacity: 0.8;">${this.downloaded ? '' : labels.silence_removed}</div>
            </div>
            <button class="close-modal" @click=${this.closeModal}>&times;</button>
          </div>
          <audio class="audio-player" src=${this.recordingUrl} controls></audio>
          <button class="download-btn" @click=${this.downloadRecording}>
            <span class="material-symbols-rounded">${this.downloaded ? 'check_circle' : 'download'}</span>
            ${this.downloaded ? labels.downloaded_msg : labels.download_btn}
          </button>
        </div>
      </div>
    `;
  }

  private saveState() {
    try {
      const arr = [...this.prompts.values()].map(p => ({
        promptId: p.promptId,
        text: p.text,
        weight: p.weight,
        cc: p.cc,
        color: p.color,
      }));
      const payload = {
        prompts: arr,
        addSlotsActive: this.addSlotsActive,
        removedSlots: [...this.removedSlots],
      };
      localStorage.setItem(this.STORAGE_KEY, JSON.stringify(payload));
    } catch { }
  }

  private handlePromptChanged(e: CustomEvent<Prompt>) {
    const { promptId, text, weight, cc } = e.detail;
    const prompt = this.prompts.get(promptId);

    if (!prompt) {
      console.error('prompt not found', promptId);
      return;
    }

    prompt.text = text;
    prompt.weight = weight;
    prompt.cc = cc;

    const newPrompts = new Map(this.prompts);
    newPrompts.set(promptId, prompt);

    this.prompts = newPrompts;
    this.requestUpdate();
    this.saveState();

    this.dispatchEvent(
      new CustomEvent('prompts-changed', { detail: this.prompts }),
    );
  }

  /** Generates radial gradients for each prompt based on weight and color, with gentle drift while playing. */
  private readonly makeBackground = throttle(
    () => {
      const clamp01 = (v: number) => Math.min(Math.max(v, 0), 1);

      const MAX_WEIGHT = 0.5;
      const MAX_ALPHA = 0.6;

      const t = performance.now() * 0.0006; // time base for gentle drift

      const bg: string[] = [];

      [...this.prompts.values()].forEach((p, i) => {
        // Stable alpha and size based on weight (no level-based pulsing)
        const alphaPct = clamp01(p.weight / MAX_WEIGHT) * MAX_ALPHA;
        const alpha = Math.round(alphaPct * 0xff)
          .toString(16)
          .padStart(2, '0');

        const stop = p.weight / 2;

        // Base grid position
        const gx = (i % 6) / 5;
        const gy = Math.floor(i / 6) / 3;

        // Gentle, eased drift per prompt scaled by driftStrength
        const phase = i * 1.37; // unique-ish per index
        const driftAmp = 4 * (this.driftStrength || 0); // percent units
        const driftX = Math.sin(t + phase) * driftAmp;
        const driftY = Math.cos(t * 0.9 + phase) * driftAmp;
        const xPct = gx * 100 + driftX;
        const yPct = gy * 100 + driftY;

        const s = `radial-gradient(circle at ${xPct}% ${yPct}%, ${p.color}${alpha} 0px, ${p.color}00 ${Math.max(0, Math.min(100, stop * 100))}%)`;

        bg.push(s);
      });

      return bg.join(', ');
    },
    30, // don't re-render more than once every XXms
  );

  public async setShowMidi(show: boolean) {
    this.showMidi = show;
    if (!this.showMidi) return;
    try {
      const inputIds = await this.midiDispatcher.getMidiAccess();
      this.midiInputIds = inputIds;
      this.activeMidiInputId = this.midiDispatcher.activeMidiInputId;
      // Notify listeners (iframe bridge) that inputs are available/updated
      this.dispatchEvent(new CustomEvent('midi-inputs-changed', { detail: { inputs: this.midiInputIds, activeId: this.activeMidiInputId } }));
    } catch (e) {
      this.showMidi = false;
      this.dispatchEvent(new CustomEvent('error', { detail: (e as any).message }));
    }
  }

  // Public API used by parent (main app) via postMessage bridge
  public getShowMidi(): boolean { return this.showMidi; }
  public async refreshMidiInputs(): Promise<void> {
    try {
      const inputIds = await this.midiDispatcher.getMidiAccess();
      this.midiInputIds = inputIds;
      this.activeMidiInputId = this.midiDispatcher.activeMidiInputId;
      this.dispatchEvent(new CustomEvent('midi-inputs-changed', { detail: { inputs: this.midiInputIds, activeId: this.activeMidiInputId } }));
    } catch (e) {
      this.dispatchEvent(new CustomEvent('error', { detail: (e as any).message }));
    }
  }
  public getMidiInputs(): string[] { return this.midiInputIds; }
  public getActiveMidiInputId(): string | null { return this.activeMidiInputId; }
  public setActiveMidiInputId(id: string) {
    if (!id) return;
    this.activeMidiInputId = id;
    this.midiDispatcher.activeMidiInputId = id;
    this.dispatchEvent(new CustomEvent('midi-inputs-changed', { detail: { inputs: this.midiInputIds, activeId: this.activeMidiInputId } }));
    this.requestUpdate();
  }

  // Localized placeholder text
  private trPlaceholder(): string {
    return LOCALES[this.lang as Lang].prompt_placeholder;
  }

  private playPause(e: Event) {
    // Prevent the bubbling play-pause event from also reaching outer listeners
    e.stopPropagation();

    // Debounce rapid clicks to avoid double toggles
    const now = Date.now();
    if (now < this.clickCooldownUntil) return;
    this.clickCooldownUntil = now + 500;

    const morphEl = this.renderRoot?.querySelector('play-pause-morph') as HTMLElement | null;

    // If currently playing or loading: this click means STOP
    if (this.playbackState === 'playing' || this.playbackState === 'loading') {
      this.lastUserAction = 'pause';
      this.optimisticPlaying = false; // pause -> play morph immediately
      this.optimisticLoading = false; // ensure spinner is off
      morphEl?.removeAttribute('loading');
      morphEl?.setAttribute('playing', 'false');
      this.dispatchEvent(new CustomEvent('pause', { bubbles: true })); // explicit pause/stop
      return;
    }

    // If paused/stopped: this click means PLAY
    if (!this.apiKeySet) {
      this.dispatchEvent(new CustomEvent('error', { detail: 'Please set your Gemini API key in the main app first.' }));
      return;
    }
    this.lastUserAction = 'play';
    this.optimisticLoading = true; // show spinner immediately
    this.optimisticPlaying = null; // follow real state for icon
    morphEl?.setAttribute('loading', '');
    this.dispatchEvent(new CustomEvent('play', { bubbles: true }));
  }

  public addFilteredPrompt(prompt: string) {
    this.filteredPrompts = new Set([...this.filteredPrompts, prompt]);
  }

  public setPromptLabels(labels: string[]) {
    const updated = new Map<string, Prompt>();
    let i = 0;
    for (const [key, p] of this.prompts.entries()) {
      const newText = labels[i] ?? p.text;
      updated.set(key, { ...p, text: newText });
      i++;
    }
    this.prompts = updated;
    this.requestUpdate();
    this.dispatchEvent(new CustomEvent('prompts-changed', { detail: this.prompts }));
  }

  public getPrompts(): Map<string, Prompt> {
    return new Map(this.prompts);
  }

  private addExtraSlot(idx: number) {
    const promptId = `extra-${idx}`;
    if (this.prompts.has(promptId)) return;
    const color = ['#9900ff', '#2af6de', '#ff25f6', '#ffdd28'][idx % 4];
    const p: Prompt = { promptId, text: this.trPlaceholder(), weight: 0, cc: 100 + idx, color };
    const updated = new Map(this.prompts);
    updated.set(promptId, p);
    this.prompts = updated;
    const slots = [...this.addSlotsActive];
    slots[idx] = true;
    this.addSlotsActive = slots;
    this.requestUpdate();
    this.saveState();
    this.dispatchEvent(new CustomEvent('prompts-changed', { detail: this.prompts }));
  }

  private addBaseSlot(idx: number) {
    const id = this.baseOrder[idx];
    if (!id || this.prompts.has(id)) return;
    const base = this.basePrompts.get(id);
    if (!base) return;
    const updated = new Map(this.prompts);
    updated.set(id, { ...base });
    this.prompts = updated;
    const rem = new Set(this.removedSlots);
    rem.delete(id);
    this.removedSlots = rem;
    this.requestUpdate();
    this.saveState();
    this.dispatchEvent(new CustomEvent('prompts-changed', { detail: this.prompts }));
  }

  private clearPrompt(promptId: string) {
    if (!this.prompts.has(promptId)) return;
    if (promptId.startsWith('extra-')) {
      // Remove extra prompt and deactivate slot
      const idx = Number(promptId.split('-')[1] || 0);
      const updated = new Map(this.prompts);
      updated.delete(promptId);
      this.prompts = updated;
      const slots = [...this.addSlotsActive];
      if (!Number.isNaN(idx)) slots[idx] = false;
      this.addSlotsActive = slots;
      this.requestUpdate();
      this.saveState();
      this.dispatchEvent(new CustomEvent('prompts-changed', { detail: this.prompts }));
      return;
    }
    // Remove built-in prompt and mark slot as removed to render add button in-grid
    const updated = new Map(this.prompts);
    updated.delete(promptId);
    this.prompts = updated;
    const rem = new Set(this.removedSlots);
    rem.add(promptId);
    this.removedSlots = rem;
    this.requestUpdate();
    this.saveState();
    this.dispatchEvent(new CustomEvent('prompts-changed', { detail: this.prompts }));
  }

  public resetAll() {
    // Reset to original base prompts
    // BUT maintain the "22 active, 2 empty" rule
    const newPrompts = new Map<string, Prompt>();
    let count = 0;
    for (const [k, p] of this.basePrompts.entries()) {
      if (count < 22) {
        newPrompts.set(k, { ...p });
      }
      count++;
    }
    this.prompts = newPrompts;
    this.addSlotsActive = [false, false, false, false];
    this.removedSlots = new Set();
    this.requestUpdate();
    this.saveState();
    this.dispatchEvent(new CustomEvent('prompts-changed', { detail: this.prompts }));
  }

  private formatDuration(sec: number) {
    if (!sec) return "0:00";
    const m = Math.floor(sec / 60);
    const s = Math.floor(sec % 60);
    return `${m}:${s.toString().padStart(2, '0')}`;
  }

  private toggleRecording() {
    if (this.recordInterval) {
      clearInterval(this.recordInterval);
      this.recordInterval = null;
    }

    this.isRecording = !this.isRecording;
    if (this.isRecording) {
      this.recordElapsed = 0;
      this.recordAudioElapsed = 0;
      const startTime = Date.now();
      let lastTick = startTime;

      this.recordInterval = window.setInterval(() => {
        const now = Date.now();
        const dt = (now - lastTick) / 1000;
        lastTick = now;

        this.recordElapsed = (now - startTime) / 1000;
        // Sensitivity threshold bumped to 0.02
        if (this.audioLevel > 0.02) {
          this.recordAudioElapsed += dt;
        }
        this.requestUpdate();
      }, 100);

      this.dispatchEvent(new CustomEvent('start-recording'));
    } else {
      this.dispatchEvent(new CustomEvent('stop-recording'));
    }
  }

  private toggleMidiPanel() {
    this.setShowMidi(!this.showMidi);
  }

  private handleVolumeChange(e: Event) {
    const val = parseFloat((e.target as HTMLInputElement).value);
    this.volume = val;

    // 1. IPC to native
    if ((window as any).ipc) {
      (window as any).ipc.postMessage('set_volume:' + val);
    }

    // 2. Global variable for hooks
    (window as any)._currentVolume = val;

    // 3. Audio tags
    document.querySelectorAll('audio, video').forEach((el) => {
      (el as HTMLMediaElement).volume = val;
    });

    // 4. Captured AudioContext Gains (from mod.rs hook)
    const gains = (window as any)._activeMasterGains;
    if (Array.isArray(gains)) {
      gains.forEach((g: any) => {
        try {
          if (g && g.gain) {
            // smooth transition
            g.gain.setTargetAtTime(val, g.context.currentTime, 0.1);
          }
        } catch {
          if (g && g.gain) g.gain.value = val;
        }
      });
    }
  }


  protected updated(changedProps: Map<string, any>) {
    if (changedProps.has('playbackState')) {
      const state = this.playbackState;

      // Set drift target based on state and ensure the animation loop is running
      this.driftTarget = (state === 'playing' || state === 'loading') ? 1 : 0;
      this.ensureDriftLoop();

      if (this.lastUserAction === 'play') {
        if (state === 'playing') {
          this.optimisticLoading = false;
          this.optimisticPlaying = null;
          this.lastUserAction = null;
        } else if (state === 'loading') {
          this.optimisticLoading = true;
        } else if (state === 'paused' || state === 'stopped') {
          this.optimisticLoading = true;
        }
      } else if (this.lastUserAction === 'pause') {
        this.optimisticLoading = false;
        this.optimisticPlaying = false;
        if (state === 'paused' || state === 'stopped') {
          this.lastUserAction = null;
        }
      } else {
        this.optimisticLoading = (state === 'loading');
        this.optimisticPlaying = null;
      }
    }
  }

  private ensureDriftLoop() {
    if (this.driftRaf != null) return;
    this.lastDriftTick = performance.now();
    const tick = () => {
      const now = performance.now();
      const dt = Math.max(0, now - this.lastDriftTick) / 1000; // seconds
      this.lastDriftTick = now;

      // Approach driftTarget smoothly (exponential smoothing)
      const speed = 3.0; // higher = faster return/engage
      const diff = this.driftTarget - this.driftStrength;
      const step = 1 - Math.exp(-speed * dt);
      this.driftStrength = this.driftStrength + diff * step;

      // Force a re-render so gradients animate (uses performance.now in makeBackground)
      this.requestUpdate();

      // If we're returning to base and very close, stop the loop; otherwise keep running
      if (this.driftTarget === 0 && Math.abs(this.driftStrength) < 0.001) {
        this.driftStrength = 0;
        this.driftRaf = null;
        return;
      }
      this.driftRaf = requestAnimationFrame(tick);
    };
    this.driftRaf = requestAnimationFrame(tick);
  }

  override render() {
    const bg = styleMap({
      backgroundImage: this.makeBackground(),
    });
    const playingProp = this.optimisticPlaying !== null
      ? this.optimisticPlaying
      : (this.playbackState === 'playing');
    const loadingProp = this.optimisticLoading || this.playbackState === 'loading';

    return html`<div id="background" style=${bg}></div>
      ${this.renderModal()}
      <div id="content">
        <div id="gridWrap">
          <div id="grid">${this.renderPrompts()}</div>
        </div>
        <div id="sideControls">
          <!-- Volume at the top -->
          <div class="volume-container" title="Master Volume">
            <span class="material-symbols-rounded volume-icon">
              ${this.volume <= 0.001 ? 'volume_off' : this.volume < 0.5 ? 'volume_down' : 'volume_up'}
            </span>
            <input type="range" class="volume-slider" min="0" max="1" step="0.01"
              .value=${this.volume.toString()}
              @input=${this.handleVolumeChange}>
          </div>

          <play-pause-morph
            ?playing=${playingProp}
            ?loading=${loadingProp}
            @play-pause=${this.playPause}
          ></play-pause-morph>

          <div class="mini-controls">
             <!-- MIDI Toggle -->
             <button class="mini-btn ${this.showMidi ? 'toggled' : ''}" @click=${this.toggleMidiPanel} title=${LOCALES[this.lang as Lang].midi_tooltip}>
               <span class="material-symbols-rounded">piano</span>
             </button>

             <!-- Record Toggle -->
             <button class="mini-btn ${this.isRecording ? 'active' : ''}" @click=${this.toggleRecording} title="${this.isRecording ? LOCALES[this.lang as Lang].stop_tooltip : LOCALES[this.lang as Lang].record_tooltip}">
               <span class="material-symbols-rounded">${this.isRecording ? 'stop' : 'radio_button_checked'}</span>
             </button>

             <!-- Reset -->
             <button class="mini-btn" @click=${() => this.resetAll()} title=${LOCALES[this.lang as Lang].reset_tooltip}>
               <span class="material-symbols-rounded">restart_alt</span>
             </button>
          </div>

          <div class="rec-timer-container">
            ${this.isRecording ? html`
               <div class="rec-timer-elapsed">${this.formatDuration(this.recordElapsed)}</div>
               <div class="rec-timer-audio">Audio: ${this.formatDuration(this.recordAudioElapsed)}</div>
            ` : html``}
          </div>
        </div>
      </div>`;
  }

  private renderPromptWithClear(promptId: string) {
    const p = this.prompts.get(promptId);
    if (!p) return html``;
    return html`<div class="pc-wrap">
      <button class="pc-clear" title=${LOCALES[this.lang as Lang].clear_tooltip} @click=${() => this.clearPrompt(promptId)}>
        <span class="material-symbols-rounded">close</span>
      </button>
      <prompt-controller
        promptId=${p.promptId}
        ?filtered=${this.filteredPrompts.has(p.text)}
        cc=${p.cc}
        text=${p.text}
        weight=${p.weight}
        color=${p.color}
        lang=${this.lang}
        .midiDispatcher=${this.midiDispatcher}
        .showCC=${this.showMidi}
        audioLevel=${this.audioLevel}
        @prompt-changed=${this.handlePromptChanged}
      ></prompt-controller>
    </div>`;
  }

  private renderPrompts() {
    const nodes: any[] = [];
    // Render in base grid order, allowing removed slots to show an add button
    this.baseOrder.forEach((id, idx) => {
      const p = this.prompts.get(id);
      if (!p || this.removedSlots.has(id)) {
        nodes.push(html`<button class="add-slot" @click=${() => this.addBaseSlot(idx)} title=${LOCALES[this.lang as Lang].add_tooltip}>
          <span class="material-symbols-rounded add-icon">add</span>
        </button>`);
      } else {
        nodes.push(this.renderPromptWithClear(id));
      }
    });
    return nodes;
  }
}
