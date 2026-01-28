
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

  async exportAndDownload(options: ExportOptions & { audioFilePath: string; audio?: HTMLAudioElement | null }) {
    if (this.isExporting) {
      throw new Error('Export already in progress');
    }
    this.isExporting = true;

    // Destructure options
    const { video, segment, backgroundConfig, mousePositions, speed = 1, audioFilePath, audio } = options;

    const preset = DIMENSION_PRESETS[options.dimensions] || DIMENSION_PRESETS['1080p'];
    let width = preset.width;
    let height = preset.height;

    // Get raw video dimensions
    const vidW = video?.videoWidth || 1920;
    const vidH = video?.videoHeight || 1080;

    // Calculate CROPPED dimensions (matching preview canvas behavior)
    const crop = segment?.crop || { x: 0, y: 0, width: 1, height: 1 };
    const croppedW = Math.round(vidW * crop.width);
    const croppedH = Math.round(vidH * crop.height);

    console.log('[Exporter] Video dimensions:', vidW, 'x', vidH);
    console.log('[Exporter] Crop rect:', crop);
    console.log('[Exporter] Cropped dimensions:', croppedW, 'x', croppedH);
    console.log('[Exporter] Dimension preset:', options.dimensions, '→', preset);

    if (preset.height === 0) {
      // Original: Use exact cropped dimensions
      width = croppedW;
      height = croppedH;
    } else {
      // 1080p / 720p: Fix the height, adjust width to maintain aspect ratio
      // This allows vertical videos to remain vertical, just scaled to 1080p height
      height = preset.height;
      width = Math.round(height * (croppedW / croppedH));
    }

    // Ensure dimensions are even (required for ffmpeg yuv420p)
    if (width % 2 !== 0) width--;
    if (height % 2 !== 0) height--;

    console.log('[Exporter] Final export dimensions:', width, 'x', height);

    const fps = 60;

    // Convert video blob to Uint8Array for Rust
    let videoDataArray: number[] | null = null;
    let audioDataArray: number[] | null = null;

    // Check if we are using a blob URL (loaded project) or a local file path
    // Since we don't have direct access to the file path in loaded projects, we fetch the blob.
    if (video && video.src && video.src.startsWith('blob:')) {
      console.log('[Exporter] Fetching video blob data...');
      try {
        const resp = await fetch(video.src);
        const blob = await resp.blob();
        const buffer = await blob.arrayBuffer();
        // Convert to regular array for JSON serialization (Rust Vec<u8>)
        // Note: This might be slow for huge files. 
        // Better approach for production: Tauri sidecar or streaming, but simple array works for <500MB
        videoDataArray = Array.from(new Uint8Array(buffer));
      } catch (e) {
        console.error("Failed to extract video data", e);
        throw new Error("Failed to prepare video for export");
      }
    }

    // Also fetch audio blob if audio element has a blob URL and no file path provided
    if (audio && audio.src && audio.src.startsWith('blob:') && !audioFilePath) {
      console.log('[Exporter] Fetching audio blob data...');
      try {
        const resp = await fetch(audio.src);
        const blob = await resp.blob();
        const buffer = await blob.arrayBuffer();
        audioDataArray = Array.from(new Uint8Array(buffer));
      } catch (e) {
        console.error("Failed to extract audio data", e);
        // Audio is optional, don't throw
      }
    }

    // Prepare config payload
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
      mousePositions: mousePositions,
      // NEW: Pass video data if we are exporting a loaded project
      videoData: videoDataArray,
      // NEW: Pass audio data if we are exporting a loaded project
      audioData: audioDataArray,
      // If null, Rust will fallback to the global VIDEO_PATH/audio_path (fresh recording)
    };

    // @ts-ignore
    const { invoke } = window.__TAURI__.core;

    try {
      console.log('[Exporter] Sending to native backend...');
      const res = await invoke('start_export_server', exportConfig);
      console.log('Export Success:', res);
    } catch (e) {
      console.error("Native Export Failed:", e);
      throw e;
    } finally {
      this.isExporting = false;
    }
  }

  cancel() {
    // Cancellation logic
  }
}

export const videoExporter = new VideoExporter();