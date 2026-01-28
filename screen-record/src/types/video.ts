export type ExportQuality = 'original' | 'balanced';
export type DimensionPreset = 'original' | '1080p' | '720p';

export interface ZoomKeyframe {
  time: number;
  duration: number;
  zoomFactor: number;
  positionX: number;
  positionY: number;
  easingType: 'linear' | 'easeOut' | 'easeInOut';
}

export interface TextSegment {
  id: string;
  startTime: number;
  endTime: number;
  text: string;
  style: {
    fontSize: number;
    color: string;
    x: number;  // 0-100 percentage
    y: number;  // 0-100 percentage
  };
}

export interface CropRect {
  x: number; // 0-1
  y: number; // 0-1
  width: number; // 0-1
  height: number; // 0-1
}

export interface VideoSegment {
  trimStart: number;
  trimEnd: number;
  zoomKeyframes: ZoomKeyframe[];
  smoothMotionPath?: { time: number; x: number; y: number; zoom: number }[];
  zoomInfluencePoints?: { time: number; value: number }[];
  textSegments: TextSegment[];
  crop?: CropRect;
}

export interface BackgroundConfig {
  scale: number;
  borderRadius: number;
  backgroundType: 'solid' | 'gradient1' | 'gradient2' | 'gradient3' | 'custom';
  shadow?: number;
  cursorScale?: number;
  cursorSmoothness?: number;
  customBackground?: string;
  cropBottom?: number; // 0-100 percentage
  volume?: number; // 0-1
}

export interface MousePosition {
  x: number;
  y: number;
  timestamp: number;
  isClicked?: boolean;
  cursor_type?: string;
}

export interface VideoMetadata {
  total_chunks: number;
  duration: number;
  width: number;
  height: number;
}

export interface ExportOptions {
  quality?: ExportQuality;
  dimensions: DimensionPreset;
  speed: number;
  video?: HTMLVideoElement;
  canvas?: HTMLCanvasElement;
  tempCanvas?: HTMLCanvasElement;
  segment?: VideoSegment;
  backgroundConfig?: BackgroundConfig;
  mousePositions?: MousePosition[];
  onProgress?: (progress: number) => void;
  audio?: HTMLAudioElement;
}

export interface ExportPreset {
  width: number;
  height: number;
  bitrate: number;
  label: string;
}

export interface Project {
  id: string;
  name: string;
  createdAt: number;
  lastModified: number;
  videoBlob: Blob;
  audioBlob?: Blob;
  segment: VideoSegment;
  backgroundConfig: BackgroundConfig;
  mousePositions: MousePosition[];
  thumbnail?: string;
}