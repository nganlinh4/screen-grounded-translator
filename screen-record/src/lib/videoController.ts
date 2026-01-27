import { videoRenderer } from './videoRenderer';
import type { VideoSegment, BackgroundConfig, MousePosition } from '@/types/video';

interface VideoControllerOptions {
  videoRef: HTMLVideoElement;
  audioRef?: HTMLAudioElement;
  canvasRef: HTMLCanvasElement;
  tempCanvasRef: HTMLCanvasElement;
  onTimeUpdate?: (time: number) => void;
  onPlayingChange?: (isPlaying: boolean) => void;
  onVideoReady?: (ready: boolean) => void;
  onError?: (error: string) => void;
  onDurationChange?: (duration: number) => void;
  onMetadataLoaded?: (metadata: { duration: number, width: number, height: number }) => void;
}

interface VideoState {
  isPlaying: boolean;
  isReady: boolean;
  isSeeking: boolean;
  currentTime: number;
  duration: number;
}

interface RenderOptions {
  segment: VideoSegment;
  backgroundConfig: BackgroundConfig;
  mousePositions: MousePosition[];
}


export class VideoController {
  private video: HTMLVideoElement;
  private audio?: HTMLAudioElement;
  private canvas: HTMLCanvasElement;
  private tempCanvas: HTMLCanvasElement;
  private options: VideoControllerOptions;
  private state: VideoState;
  private renderOptions?: RenderOptions;

  constructor(options: VideoControllerOptions) {
    this.video = options.videoRef;
    this.audio = options.audioRef;
    this.canvas = options.canvasRef;
    this.tempCanvas = options.tempCanvasRef;
    this.options = options;

    this.state = {
      isPlaying: false,
      isReady: false,
      isSeeking: false,
      currentTime: 0,
      duration: 0
    };

    this.initializeEventListeners();
  }

  private initializeEventListeners() {
    console.log('[VideoController] Initializing listeners (v2-no-waiting)');
    this.video.addEventListener('loadeddata', this.handleLoadedData);
    this.video.addEventListener('play', this.handlePlay);
    this.video.addEventListener('pause', this.handlePause);
    this.video.addEventListener('timeupdate', this.handleTimeUpdate);
    this.video.addEventListener('seeked', this.handleSeeked);
    this.video.addEventListener('loadedmetadata', this.handleLoadedMetadata);
    this.video.addEventListener('durationchange', this.handleDurationChange);
    this.video.addEventListener('error', this.handleError);
  }

  private handleLoadedData = () => {
    console.log('[VideoController] Video loaded data');
    this.renderFrame();
    this.setReady(true);
  };

  private handlePlay = () => {
    console.log('[VideoController] Play event');
    if (this.audio) {
      this.audio.currentTime = this.video.currentTime;
      this.audio.playbackRate = this.video.playbackRate;
      this.audio.play().catch(e => console.warn('[VideoController] Audio play failed:', e));
    }

    // Ensure animation is running
    if (this.renderOptions) {
      videoRenderer.startAnimation({
        video: this.video,
        canvas: this.canvas,
        tempCanvas: this.tempCanvas,
        segment: this.renderOptions.segment,
        backgroundConfig: this.renderOptions.backgroundConfig,
        mousePositions: this.renderOptions.mousePositions,
        currentTime: this.video.currentTime
      });
    }

    this.setPlaying(true);
  };

  private handlePause = () => {
    console.log('[VideoController] Pause event');
    if (this.audio) {
      this.audio.pause();
    }
    this.setPlaying(false);
    this.renderFrame();
  };

  private handleTimeUpdate = () => {
    if (!this.state.isSeeking) {
      const currentTime = this.video.currentTime;

      // Handle trim bounds
      if (this.renderOptions?.segment) {
        const { trimStart, trimEnd } = this.renderOptions.segment;

        if (currentTime >= trimEnd && !this.video.paused) {
          console.log('[VideoController] Reached trim end', { currentTime, trimStart, trimEnd });
          this.video.pause(); // Triggers handlePause
          return;
        }

        if (currentTime < trimStart) {
          this.video.currentTime = trimStart;
          if (this.audio) this.audio.currentTime = trimStart;
        }
      }

      // Smooth audio sync: only correct if drift > 150ms to avoid audio stutter
      if (this.audio && !this.video.paused) {
        const drift = Math.abs(this.video.currentTime - this.audio.currentTime);
        if (drift > 0.15) {
          this.audio.currentTime = this.video.currentTime;
        }
      }

      this.setCurrentTime(currentTime);
      // Removed renderFrame here - allow animation loop to handle updates during playback
      // If paused, handlePause triggers renderFrame.
      // If playing, startAnimation loop handles it.
    }
  };

  private handleSeeked = () => {
    this.setSeeking(false);
    this.setCurrentTime(this.video.currentTime);
    this.renderFrame();
  };

  private handleLoadedMetadata = () => {
    console.log('Video metadata loaded:', {
      duration: this.video.duration,
      width: this.video.videoWidth,
      height: this.video.videoHeight
    });

    this.options.onMetadataLoaded?.({
      duration: this.video.duration,
      width: this.video.videoWidth,
      height: this.video.videoHeight
    });

    if (this.video.duration !== Infinity) {
      this.setDuration(this.video.duration);
      // Initialize segment if none exists
      if (!this.renderOptions?.segment) {
        this.renderOptions = {
          segment: this.initializeSegment(),
          backgroundConfig: {
            scale: 100,
            borderRadius: 8,
            backgroundType: 'solid'
          },
          mousePositions: []
        };
      }
    }
  };

  private handleDurationChange = () => {
    console.log('[VideoController] Duration changed:', this.video.duration);
    if (this.video.duration !== Infinity) {
      this.setDuration(this.video.duration);

      // Update trimEnd if it was not set correctly or is 0
      if (this.renderOptions?.segment) {
        if (this.renderOptions.segment.trimEnd === 0 || this.renderOptions.segment.trimEnd > this.video.duration) {
          console.log('[VideoController] Updating segment trimEnd to duration:', this.video.duration);
          this.renderOptions.segment.trimEnd = this.video.duration;
        }
      }
    }
  };

  private handleError = (error: ErrorEvent) => {
    console.error('Video error:', error);
    this.options.onError?.(error.message);
  };

  private setPlaying(playing: boolean) {
    this.state.isPlaying = playing;
    this.options.onPlayingChange?.(playing);
  }

  private setReady(ready: boolean) {
    this.state.isReady = ready;
    this.options.onVideoReady?.(ready);
  }

  private setSeeking(seeking: boolean) {
    this.state.isSeeking = seeking;
  }

  private setCurrentTime(time: number) {
    this.state.currentTime = time;
    this.options.onTimeUpdate?.(time);
  }

  private setDuration(duration: number) {
    this.state.duration = duration;
    this.options.onDurationChange?.(duration);
  }

  private renderFrame() {
    if (!this.renderOptions) return;

    const renderContext = {
      video: this.video,
      canvas: this.canvas,
      tempCanvas: this.tempCanvas,
      segment: this.renderOptions.segment,
      backgroundConfig: this.renderOptions.backgroundConfig,
      mousePositions: this.renderOptions.mousePositions,
      currentTime: this.getAdjustedTime(this.video.currentTime)
    };

    // Only draw if video is ready
    if (this.video.readyState >= 2) {
      // Draw even if paused to support live preview when editing
      // but we can skip if the video is at the end and paused
      if (renderContext.video.paused && renderContext.video.currentTime >= renderContext.video.duration) {
        // No animationFrame here, as renderFrame is called manually or by event listeners
        return;
      }
      // Update the active context for the animation loop
      videoRenderer.updateRenderContext(renderContext);
      videoRenderer.drawFrame(renderContext);
    } else {
      // console.log('[VideoController] Skipping frame - video not ready');
    }
  }

  // Public API
  public updateRenderOptions(options: RenderOptions) {
    this.renderOptions = options;
    this.renderFrame();
  }

  public play() {
    // Reset seeking state to ensure play works after seek
    this.setSeeking(false);

    if (!this.state.isReady) {
      console.warn('[VideoController] Play ignored: not ready');
      return;
    }

    if (this.renderOptions?.segment) {
      const { trimStart, trimEnd } = this.renderOptions.segment;
      if (this.video.currentTime >= trimEnd - 0.05) {
        this.video.currentTime = trimStart;
      }
    }

    this.video.play().catch(e => console.warn('[VideoController] Play attempt failed:', e));
  }

  public pause() {
    this.video.pause();
  }

  public seek(time: number) {
    if (!this.state.isReady) return;

    if (this.renderOptions?.segment) {
      const { trimStart, trimEnd } = this.renderOptions.segment;
      time = Math.max(trimStart, Math.min(time, trimEnd));
    }

    this.setSeeking(true);
    this.video.currentTime = time;
    if (this.audio) this.audio.currentTime = time;

    // IMPORTANT: Do NOT call renderFrame() here. 
    // Let the 'seeked' event trigger it to avoid drawing frames mid-seek
    // which can cause artifacts/corruption.
  }

  public togglePlayPause() {
    if (this.video.paused) {
      this.play();
    } else {
      this.pause();
    }
  }

  public setVolume(volume: number) {
    if (this.audio) {
      this.audio.volume = volume;
    }
    this.video.volume = volume;
  }

  public destroy() {
    this.video.removeEventListener('loadeddata', this.handleLoadedData);
    this.video.removeEventListener('play', this.handlePlay);
    this.video.removeEventListener('pause', this.handlePause);
    this.video.removeEventListener('timeupdate', this.handleTimeUpdate);
    this.video.removeEventListener('seeked', this.handleSeeked);
    this.video.removeEventListener('loadedmetadata', this.handleLoadedMetadata);
    this.video.removeEventListener('durationchange', this.handleDurationChange);
    this.video.removeEventListener('error', this.handleError);
  }

  // Getters
  public get isPlaying() { return this.state.isPlaying; }
  public get isReady() { return this.state.isReady; }
  public get isSeeking() { return this.state.isSeeking; }
  public get currentTime() { return this.state.currentTime; }
  public get duration() { return this.state.duration; }

  // Add this new method
  public async loadVideo({ videoBlob, videoUrl, onLoadingProgress }: { videoBlob?: Blob, videoUrl?: string, onLoadingProgress?: (p: number) => void }): Promise<string> {
    try {
      // Reset states
      this.setReady(false);
      this.setSeeking(false);
      this.setPlaying(false);

      // Clear previous video properly to avoid 'Video error' events
      this.video.pause();
      this.video.src = "";
      this.video.load();
      this.video.removeAttribute('src');

      // Clear previous audio
      if (this.audio) {
        this.audio.pause();
        this.audio.src = "";
        this.audio.load();
        this.audio.removeAttribute('src');
      }

      let blob: Blob;

      if (videoBlob) {
        blob = videoBlob;
      } else if (videoUrl) {
        console.log('[VideoController] Fetching video data from:', videoUrl);
        const response = await fetch(videoUrl);
        if (!response.ok) throw new Error('Failed to fetch video');

        const reader = response.body!.getReader();
        const contentLength = +(response.headers.get('Content-Length') ?? 0);
        let receivedLength = 0;
        const chunks = [];

        while (true) {
          const { done, value } = await reader.read();
          if (done) break;
          chunks.push(value);
          receivedLength += value.length;
          const progress = Math.min(((receivedLength / contentLength) * 100), 100);
          onLoadingProgress?.(progress);
        }

        blob = new Blob(chunks, { type: 'video/mp4' });
      } else {
        throw new Error('No video data provided');
      }

      const objectUrl = URL.createObjectURL(blob);
      await this.handleVideoSourceChange(objectUrl);
      return objectUrl;
    } catch (error) {
      console.error('[VideoController] Failed to load video:', error);
      throw error;
    }
  }

  public async loadAudio({ audioBlob, audioUrl, onLoadingProgress }: { audioBlob?: Blob, audioUrl?: string, onLoadingProgress?: (p: number) => void }): Promise<string> {
    try {
      if (!this.audio) return "";

      let blob: Blob;

      if (audioBlob) {
        blob = audioBlob;
      } else if (audioUrl) {
        console.log('[VideoController] Fetching audio data from:', audioUrl);
        const response = await fetch(audioUrl);
        if (!response.ok) throw new Error('Failed to fetch audio');

        const reader = response.body!.getReader();
        const contentLength = +(response.headers.get('Content-Length') ?? 0);
        let receivedLength = 0;
        const chunks = [];

        while (true) {
          const { done, value } = await reader.read();
          if (done) break;
          chunks.push(value);
          receivedLength += value.length;
          const progress = Math.min(((receivedLength / contentLength) * 100), 100);
          onLoadingProgress?.(progress);
        }

        blob = new Blob(chunks, { type: 'audio/wav' });
      } else {
        return "";
      }

      const objectUrl = URL.createObjectURL(blob);
      this.audio.src = objectUrl;
      this.audio.load();

      return objectUrl;
    } catch (error) {
      console.error('[VideoController] Failed to load audio:', error);
      return "";
    }
  }

  // Update existing method to be private
  private async handleVideoSourceChange(videoUrl: string): Promise<void> {
    if (!this.video || !this.canvas) return;

    // Reset states
    this.setReady(false);
    this.setSeeking(false);
    this.setPlaying(false);

    // Reset video element
    this.video.pause();
    this.video.src = "";
    this.video.load();
    this.video.removeAttribute('src');

    return new Promise<void>((resolve) => {
      const handleCanPlayThrough = () => {
        console.log('[VideoController] Video can play through');
        this.video.removeEventListener('canplaythrough', handleCanPlayThrough);

        // Set up canvas
        this.canvas.width = this.video.videoWidth;
        this.canvas.height = this.video.videoHeight;

        const ctx = this.canvas.getContext('2d');
        if (ctx) {
          ctx.imageSmoothingEnabled = true;
          ctx.imageSmoothingQuality = 'high';
        }

        this.setReady(true);
        resolve();
      };

      // Set up video
      this.video.addEventListener('canplaythrough', handleCanPlayThrough);
      this.video.preload = 'auto';
      this.video.src = videoUrl;
      this.video.load(); // Explicitly load the video
    });
  }

  // Add this new method to handle time adjustment
  private getAdjustedTime(time: number): number {
    if (!this.renderOptions?.segment) return time;

    const { trimStart, trimEnd } = this.renderOptions.segment;
    const trimDuration = trimEnd - trimStart;

    // Calculate the relative position within the trimmed section
    const relativeTime = ((time - trimStart) % trimDuration);

    // If time is negative, adjust it to wrap from the end
    const adjustedTime = relativeTime < 0
      ? trimEnd + relativeTime
      : trimStart + relativeTime;

    return adjustedTime;
  }

  // Add new method
  public initializeSegment(): VideoSegment {
    // If duration is available, use it, otherwise use a default safe large number
    // It will be corrected by handleDurationChange later
    const duration = (this.video && this.video.duration !== Infinity && !isNaN(this.video.duration))
      ? this.video.duration
      : 3600;

    const initialSegment: VideoSegment = {
      trimStart: 0,
      trimEnd: duration,
      zoomKeyframes: [],
      textSegments: []
    };
    return initialSegment;
  }

  // Add this new method
  public isAtEnd(): boolean {
    if (!this.renderOptions?.segment) return false;
    const { trimEnd } = this.renderOptions.segment;
    return Math.abs(this.video.currentTime - trimEnd) < 0.1; // Allow 0.1s tolerance
  }
}

export const createVideoController = (options: VideoControllerOptions) => {
  return new VideoController(options);
};