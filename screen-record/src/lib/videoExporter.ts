
import type {
  ExportOptions,
} from '@/types/video';

export const EXPORT_PRESETS: Record<string, { label: string; quality: number }> = {
  balanced: { label: 'Balanced (Recommended)', quality: 0.8 },
  original: { label: 'Original Quality', quality: 1.0 },
};

export const DIMENSION_PRESETS: Record<string, { label: string; width: number; height: number }> = {
  original: { label: 'Original Size', width: 0, height: 0 },
  '1080p': { label: 'Full HD (1080p)', width: 1920, height: 1080 },
  '720p': { label: 'HD (720p)', width: 1280, height: 720 },
};

export class VideoExporter {
  private isExporting = false;

  async exportAndDownload(options: ExportOptions & { audioFilePath: string }) {
    if (this.isExporting) {
      throw new Error('Export already in progress');
    }
    this.isExporting = true;

    const { video: sourceVideo, segment, backgroundConfig, mousePositions, speed = 1, audioFilePath } = options;

    const preset = DIMENSION_PRESETS[options.dimensions] || DIMENSION_PRESETS['1080p'];
    let width = preset.width;
    let height = preset.height;

    if (width === 0 || height === 0) {
      width = sourceVideo?.videoWidth || 1920;
      height = sourceVideo?.videoHeight || 1080;
      if (width % 2 !== 0) width--;
      if (height % 2 !== 0) height--;
    }

    const fps = 60;

    // Construct Config Payload
    const exportConfig = {
      width,
      height,
      framerate: fps,
      audioPath: audioFilePath,
      trimStart: segment?.trimStart || 0,
      duration: (segment?.trimEnd || 0) - (segment?.trimStart || 0),
      speed: speed,
      segment: segment,
      backgroundConfig: backgroundConfig,
      mousePositions: mousePositions
    };

    // @ts-ignore
    const { invoke } = window.__TAURI__.core;

    try {
      // Send to Rust for native processing
      const res = await invoke('start_export_server', exportConfig);
      console.log('Export Success:', res);

      // Optional: Open file location
      // await invoke('show_item_in_folder', { path: res.path });

    } catch (e) {
      console.error("Native Export Failed:", e);
      throw e;
    } finally {
      this.isExporting = false;
    }
  }

  cancel() {
    // Cancellation logic would need an abort signal to Rust
  }
}

export const videoExporter = new VideoExporter();