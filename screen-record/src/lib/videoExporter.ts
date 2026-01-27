import { videoRenderer } from './videoRenderer';
import type {
  ExportOptions,
  BackgroundConfig
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

const DEFAULT_BACKGROUND_CONFIG: BackgroundConfig = {
  scale: 100,
  borderRadius: 0,
  backgroundType: 'solid',
  shadow: 0,
  cursorScale: 1
};

export class VideoExporter {
  private isExporting = false;
  private shouldStop = false;

  async exportAndDownload(options: ExportOptions & { audioFilePath: string }) {
    if (this.isExporting) {
      throw new Error('Export already in progress');
    }
    this.isExporting = true;
    this.shouldStop = false;

    const { video: sourceVideo, segment, canvas, tempCanvas, backgroundConfig, mousePositions, speed = 1, audioFilePath } = options;
    if (!sourceVideo || !segment || !canvas || !tempCanvas) throw new Error('Missing required elements');

    // 1. Setup Offscreen Rendering
    const exportVideo = document.createElement('video');
    exportVideo.muted = true;
    exportVideo.playsInline = true;
    exportVideo.src = sourceVideo.src;
    exportVideo.crossOrigin = "anonymous";
    exportVideo.style.display = 'none';
    document.body.appendChild(exportVideo);

    await new Promise<void>((resolve, reject) => {
      exportVideo.onloadeddata = () => resolve();
      exportVideo.onerror = (e) => reject(e);
      exportVideo.load();
    });

    // 2. Start FFmpeg Server via Rust
    const preset = DIMENSION_PRESETS[options.dimensions] || DIMENSION_PRESETS['1080p'];
    let width = preset.width;
    let height = preset.height;

    if (width === 0 || height === 0) {
      width = sourceVideo.videoWidth;
      height = sourceVideo.videoHeight;
      // Ensure even dimensions
      if (width % 2 !== 0) width--;
      if (height % 2 !== 0) height--;
    }

    const fps = 60;

    // @ts-ignore
    const { invoke } = window.__TAURI__.core;

    let port = 0;

    try {
      const res = await invoke('start_export_server', {
        width,
        height,
        framerate: fps,
        audioPath: audioFilePath,
        trimStart: segment.trimStart,
        duration: (segment.trimEnd - segment.trimStart),
        speed: speed
      });
      port = res.port;
    } catch (e) {
      console.error("Failed to start FFmpeg server:", e);
      this.isExporting = false;
      document.body.removeChild(exportVideo);
      throw e;
    }

    // 3. Render Loop
    const dt = 1 / fps; // Source step
    const step = dt * speed;

    let currentTime = segment.trimStart;
    const endTime = segment.trimEnd;
    const serverUrl = `http://127.0.0.1:${port}`;

    // Resize canvas for export
    canvas.width = width;
    canvas.height = height;
    tempCanvas.width = width;
    tempCanvas.height = height;

    try {
      while (currentTime < endTime && !this.shouldStop) {
        // Seek
        exportVideo.currentTime = currentTime;
        await new Promise<void>(r => {
          const h = () => {
            exportVideo.removeEventListener('seeked', h);
            r();
          };
          exportVideo.addEventListener('seeked', h);
        });

        // Render
        const renderContext = {
          video: exportVideo,
          canvas,
          tempCanvas,
          segment,
          backgroundConfig: backgroundConfig || DEFAULT_BACKGROUND_CONFIG,
          mousePositions: mousePositions || [],
          currentTime: currentTime
        };

        await videoRenderer.drawFrame(renderContext, { exportMode: true });

        // Compress to JPEG (High quality, fast)
        const blob = await new Promise<Blob | null>(r => canvas.toBlob(r, 'image/jpeg', 0.95));

        if (blob) {
          // Upload to Rust
          await fetch(serverUrl, {
            method: 'POST',
            body: blob
          });
        }

        currentTime += step;
        const progress = ((currentTime - segment.trimStart) / (endTime - segment.trimStart)) * 100;
        options.onProgress?.(Math.min(progress, 99));
      }

      // Finish
      await fetch(`${serverUrl}/finish`, { method: 'POST' });

    } catch (e) {
      console.error("Export loop failed:", e);
    } finally {
      this.isExporting = false;
      document.body.removeChild(exportVideo);
    }
  }

  cancel() {
    this.shouldStop = true;
  }
}

export const videoExporter = new VideoExporter();
