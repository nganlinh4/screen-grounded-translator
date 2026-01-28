
import { useState, useRef, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Play, Pause, Video, Trash2, Search, Download, Loader2, FolderOpen, Upload, Wand2, Type, Keyboard, X, Minus, Square, Copy, AlertCircle } from "lucide-react";
import "./App.css";
import { Button } from "@/components/ui/button";
import { videoRenderer } from '@/lib/videoRenderer';
import { BackgroundConfig, VideoSegment, ZoomKeyframe, MousePosition, ExportOptions, Project, TextSegment } from '@/types/video';
import { videoExporter, EXPORT_PRESETS, DIMENSION_PRESETS } from '@/lib/videoExporter';
import { createVideoController } from '@/lib/videoController';

import { projectManager } from '@/lib/projectManager';
import { autoZoomGenerator } from '@/lib/autoZoom';
import { Timeline } from '@/components/Timeline';
import { thumbnailGenerator } from '@/lib/thumbnailGenerator';
import { useUndoRedo } from '@/hooks/useUndoRedo';
import { Crop } from "lucide-react";

// Replace the debounce utility with throttle
const useThrottle = (callback: Function, limit: number) => {
  const lastRunRef = useRef<number>(0);

  return useCallback((...args: any[]) => {
    const now = Date.now();
    if (now - lastRunRef.current >= limit) {
      callback(...args);
      lastRunRef.current = now;
    }
  }, [callback, limit]);
};

// Add these interfaces near the top of the file
interface MonitorInfo {
  id: string;
  name: string;
  width: number;
  height: number;
  x: number;
  y: number;
  is_primary: boolean;
}

interface Hotkey {
  code: number;
  name: string;
  modifiers: number;
}

// Add this helper function near the top of the file
const sortMonitorsByPosition = (monitors: MonitorInfo[]) => {
  return [...monitors]
    .sort((a, b) => a.x - b.x)
    .map((monitor, index) => ({
      ...monitor,
      name: `Display ${index + 1}${monitor.is_primary ? ' (Primary)' : ''}`
    }));
};

// Added helper function to calculate the range for a zoom keyframe.
// It returns an object containing the range start and end for the given keyframe.
const getKeyframeRange = (
  keyframes: ZoomKeyframe[],
  index: number
): { rangeStart: number; rangeEnd: number } => {
  const keyframe = keyframes[index];
  const prevKeyframe = index > 0 ? keyframes[index - 1] : null;
  const rangeStart =
    prevKeyframe && keyframe.time - prevKeyframe.time <= 1.0
      ? prevKeyframe.time
      : Math.max(0, keyframe.time - 1.0);
  return { rangeStart, rangeEnd: keyframe.time };
};

// FFmpeg install status type
type FfmpegInstallStatus =
  | { type: 'Idle' }
  | { type: 'Downloading'; progress: number }
  | { type: 'Extracting' }
  | { type: 'Installed' }
  | { type: 'Error'; message: string }
  | { type: 'Cancelled' };

function App() {
  const [isRecording, setIsRecording] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [currentTime, setCurrentTime] = useState(0);
  const [duration, setDuration] = useState(0);
  const [isPlaying, setIsPlaying] = useState(false);
  const { state: segment, setState: setSegment, undo, redo, canUndo, canRedo } = useUndoRedo<VideoSegment | null>(null);
  const [editingKeyframeId, setEditingKeyframeId] = useState<number | null>(null);
  const [zoomFactor, setZoomFactor] = useState(1.5);
  const [isProcessing, setIsProcessing] = useState(false);
  const [currentVideo, setCurrentVideo] = useState<string | null>(null);
  const [exportProgress, setExportProgress] = useState(0);
  const [isLoadingVideo, setIsLoadingVideo] = useState(false);
  const [loadingProgress, setLoadingProgress] = useState(0);
  const [currentAudio, setCurrentAudio] = useState<string | null>(null);
  const [isCropping, setIsCropping] = useState(false);
  const [audioFilePath, setAudioFilePath] = useState<string>("");

  // FFmpeg/ffprobe dependency check state
  const [ffmpegMissing, setFfmpegMissing] = useState(false);
  const [ffprobeMissing, setFfprobeMissing] = useState(false);
  const [isCheckingDeps, setIsCheckingDeps] = useState(true);
  const [ffmpegInstallStatus, setFfmpegInstallStatus] = useState<FfmpegInstallStatus>({ type: 'Idle' });

  const videoRef = useRef<HTMLVideoElement | null>(null);
  const audioRef = useRef<HTMLAudioElement | null>(null);
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const timelineRef = useRef<HTMLDivElement>(null);
  const previewContainerRef = useRef<HTMLDivElement>(null);

  // Add this to your App component state
  const [backgroundConfig, setBackgroundConfig] = useState<BackgroundConfig>({
    scale: 90,
    borderRadius: 48,
    backgroundType: 'gradient2',
    shadow: 100,
    volume: 1,
    cursorScale: 5
  });

  // Add this state to toggle between panels
  const [activePanel, setActivePanel] = useState<'zoom' | 'background' | 'cursor' | 'text'>('zoom');

  // Add these gradient constants
  const GRADIENT_PRESETS = {
    solid: 'bg-black',
    gradient1: 'bg-gradient-to-r from-blue-600 to-violet-600',
    gradient2: 'bg-gradient-to-r from-rose-400 to-orange-300',
    gradient3: 'bg-gradient-to-r from-emerald-500 to-teal-400'
  };

  // Add at the top of your component
  const tempCanvasRef = useRef<HTMLCanvasElement>(document.createElement('canvas'));

  // Add to your App component state
  const [mousePositions, setMousePositions] = useState<MousePosition[]>([]);

  // Add new state at the top of App component
  const [isVideoReady, setIsVideoReady] = useState(false);

  // Create video controller ref
  const videoControllerRef = useRef<ReturnType<typeof createVideoController>>();

  // Initialize controller
  useEffect(() => {
    if (!videoRef.current || !canvasRef.current) return;

    videoControllerRef.current = createVideoController({
      videoRef: videoRef.current,
      audioRef: audioRef.current || undefined,
      canvasRef: canvasRef.current,
      tempCanvasRef: tempCanvasRef.current,
      onTimeUpdate: (time) => setCurrentTime(time),
      onPlayingChange: (playing) => setIsPlaying(playing),
      onVideoReady: (ready) => setIsVideoReady(ready),
      onDurationChange: (duration) => setDuration(duration),
      onError: (error) => setError(error),
      onMetadataLoaded: (metadata) => {
        // When metadata loads, if we have a segment with invalid trimEnd (0 or > duration),
        // we must update it to match the actual duration.
        // This fixes the "Reached trim end" bug on project load.
        setSegment(prevSegment => {
          if (!prevSegment) return null;

          if (prevSegment.trimEnd === 0 || prevSegment.trimEnd > metadata.duration) {
            console.log('[App] Fixing invalid trimEnd on metadata load:', metadata.duration);
            return {
              ...prevSegment,
              trimEnd: metadata.duration
            };
          }
          return prevSegment;
        });
      }
    });

    return () => {
      videoControllerRef.current?.destroy();
    };
  }, []);

  // Check ffmpeg/ffprobe availability on mount
  useEffect(() => {
    const checkDependencies = async () => {
      try {
        const status = await invoke<{ ffmpegMissing: boolean; ffprobeMissing: boolean }>('check_ffmpeg_status');
        setFfmpegMissing(status.ffmpegMissing);
        setFfprobeMissing(status.ffprobeMissing);

        // If any dependency is missing, start installation automatically
        if (status.ffmpegMissing || status.ffprobeMissing) {
          await invoke('start_ffmpeg_install');
          // Start polling for install progress
          const pollInterval = setInterval(async () => {
            try {
              const progress = await invoke<any>('get_ffmpeg_install_progress');
              // Parse the status from Rust enum format
              if (progress === 'Idle') {
                setFfmpegInstallStatus({ type: 'Idle' });
              } else if (progress === 'Extracting') {
                setFfmpegInstallStatus({ type: 'Extracting' });
              } else if (progress === 'Installed') {
                setFfmpegInstallStatus({ type: 'Installed' });
                setFfmpegMissing(false);
                setFfprobeMissing(false);
                clearInterval(pollInterval);
              } else if (progress === 'Cancelled') {
                setFfmpegInstallStatus({ type: 'Cancelled' });
                clearInterval(pollInterval);
              } else if (typeof progress === 'object') {
                if ('Downloading' in progress) {
                  setFfmpegInstallStatus({ type: 'Downloading', progress: progress.Downloading });
                } else if ('Error' in progress) {
                  setFfmpegInstallStatus({ type: 'Error', message: progress.Error });
                  clearInterval(pollInterval);
                }
              }
            } catch (e) {
              console.error('Failed to poll install progress:', e);
            }
          }, 200);
        }
      } catch (e) {
        console.error('Failed to check ffmpeg status:', e);
      } finally {
        setIsCheckingDeps(false);
      }
    };

    checkDependencies();
  }, []);

  const handleCancelInstall = async () => {
    try {
      await invoke('cancel_ffmpeg_install');
      setFfmpegInstallStatus({ type: 'Cancelled' });
    } catch (e) {
      console.error('Failed to cancel install:', e);
    }
  };

  // Sync volume with controller
  useEffect(() => {
    if (videoControllerRef.current && backgroundConfig.volume !== undefined) {
      videoControllerRef.current.setVolume(backgroundConfig.volume);
    }
  }, [backgroundConfig.volume]);

  // Sync renderOptions with controller when segment/config changes
  useEffect(() => {
    if (!segment || !videoControllerRef.current) return;
    videoControllerRef.current.updateRenderOptions({
      segment,
      backgroundConfig,
      mousePositions
    });
  }, [segment, backgroundConfig, mousePositions]);

  // Helper function to render a frame
  const renderFrame = useCallback(() => {
    if (!segment || !videoRef.current || !canvasRef.current) return;

    // Explicitly draw frame if paused to reflect changes immediately
    if (videoRef.current.paused) {
      // If cropping, force a 1.0 zoom render AND full source (no crop) so we see the full context
      const renderSegment = isCropping ? {
        ...segment,
        crop: undefined, // Show full video
        zoomKeyframes: segment.zoomKeyframes.map(k => ({
          ...k,
          zoomFactor: 1.0,
          positionX: 0.5,
          positionY: 0.5
        }))
      } : segment;

      // Ensure we Strip all background config (Scale 100%, No padding, No shadow)
      const renderBackground = isCropping ? {
        ...backgroundConfig,
        scale: 100,
        borderRadius: 0,
        shadow: 0,
        backgroundType: 'solid' as const,
        customBackground: undefined,
        cropBottom: 0
      } : backgroundConfig;

      videoRenderer.drawFrame({
        video: videoRef.current,
        canvas: canvasRef.current,
        tempCanvas: tempCanvasRef.current,
        segment: renderSegment,
        backgroundConfig: renderBackground,
        mousePositions,
        currentTime: videoRef.current.currentTime
      });
    }
  }, [segment, backgroundConfig, mousePositions, isCropping]);

  // Remove frameCallback and simplify the animation effect
  useEffect(() => {
    const video = videoRef.current;
    if (!video) return;

    // Start animation when playing, render single frame when paused
    if (video.paused) {
      renderFrame();
    } else {
      // Logic for playing state: Also override if cropping
      const loopSegment = (isCropping && segment) ? {
        ...segment,
        crop: undefined,
        zoomKeyframes: segment.zoomKeyframes.map(k => ({ ...k, zoomFactor: 1.0, positionX: 0.5, positionY: 0.5 }))
      } : segment;

      const loopBackground = isCropping ? {
        ...backgroundConfig,
        scale: 100,
        borderRadius: 0,
        shadow: 0,
        backgroundType: 'solid' as const,
        customBackground: undefined,
        cropBottom: 0
      } : backgroundConfig;

      const renderContext = {
        video,
        canvas: canvasRef.current!,
        tempCanvas: tempCanvasRef.current,
        segment: loopSegment!,
        backgroundConfig: loopBackground,
        mousePositions,
        currentTime: video.currentTime
      };
      videoRenderer.startAnimation(renderContext);
    }

    return () => {
      videoRenderer.stopAnimation();
    };
  }, [segment, backgroundConfig, mousePositions, isCropping]);

  const [isWindowMaximized, setIsWindowMaximized] = useState(false);

  useEffect(() => {
    invoke<boolean>('is_maximized').then(setIsWindowMaximized).catch(() => { });
  }, []);

  // Update other places where drawFrame was used to use renderFrame instead
  useEffect(() => {
    if (videoRef.current && !videoRef.current.paused) return;
    renderFrame();
  }, [backgroundConfig, renderFrame]);

  // Add these state variables inside App component
  const [monitors, setMonitors] = useState<MonitorInfo[]>([]);
  const [showMonitorSelect, setShowMonitorSelect] = useState(false);


  // Add this function to fetch monitors
  const getMonitors = async () => {
    try {
      const monitors = await invoke<MonitorInfo[]>("get_monitors");
      // Sort monitors before setting state
      const sortedMonitors = sortMonitorsByPosition(monitors);
      setMonitors(sortedMonitors);
      return sortedMonitors;
    } catch (err) {
      console.error("Failed to get monitors:", err);
      setError(err as string);
      return [];
    }
  };

  const [hotkeys, setHotkeys] = useState<Hotkey[]>([]);
  const [showHotkeyDialog, setShowHotkeyDialog] = useState(false);
  const [listeningForKey, setListeningForKey] = useState(false);

  useEffect(() => {
    invoke<Hotkey[]>('get_hotkeys').then(setHotkeys).catch(() => { });
  }, []);

  const handleRemoveHotkey = async (index: number) => {
    try {
      await invoke('remove_hotkey', { index });
      setHotkeys(prev => prev.filter((_, i) => i !== index));
    } catch (err) {
      console.error("Failed to remove hotkey:", err);
    }
  };

  const [keyvizStatus, setKeyvizStatus] = useState({ installed: false, enabled: false });

  useEffect(() => {
    invoke<{ installed: boolean; enabled: boolean }>('get_keyviz_status')
      .then(setKeyvizStatus)
      .catch(console.error);
  }, []);

  const toggleKeyviz = async () => {
    try {
      if (!keyvizStatus.installed && !keyvizStatus.enabled) {
        await invoke('install_keyviz');
        await invoke('set_keyviz_enabled', { enabled: true });
        setKeyvizStatus({ installed: true, enabled: true });
      } else {
        const newEnabled = !keyvizStatus.enabled;
        await invoke('set_keyviz_enabled', { enabled: newEnabled });
        setKeyvizStatus(prev => ({ ...prev, enabled: newEnabled }));
      }
    } catch (err) {
      console.error("Failed to toggle keyviz:", err);
    }
  };

  useEffect(() => {
    if (showHotkeyDialog && listeningForKey) {
      invoke('unregister_hotkeys').catch(() => { });
      window.focus();
    } else {
      invoke('register_hotkeys').catch(() => { });
    }
    return () => {
      invoke('register_hotkeys').catch(() => { });
    };
  }, [showHotkeyDialog, listeningForKey]);

  useEffect(() => {
    if (showHotkeyDialog && listeningForKey) {
      const handleKeyDown = async (e: KeyboardEvent) => {
        e.preventDefault();

        // Ignore modifier-only presses
        if (['Control', 'Alt', 'Shift', 'Meta'].includes(e.key)) return;

        const modifiers = [];
        if (e.ctrlKey) modifiers.push('Control');
        if (e.altKey) modifiers.push('Alt');
        if (e.shiftKey) modifiers.push('Shift');
        if (e.metaKey) modifiers.push('Meta');

        try {
          const newHotkey = await invoke<Hotkey>('set_hotkey', {
            code: e.code,
            modifiers,
            key: e.key
          });
          setHotkeys(prev => [...prev, newHotkey]);
          setListeningForKey(false);
          setShowHotkeyDialog(false);
        } catch (err) {
          console.error("Failed to set hotkey:", err);
          setError(err as string || "Failed to set hotkey");
          setListeningForKey(false);
        }
      };

      window.addEventListener('keydown', handleKeyDown);
      return () => window.removeEventListener('keydown', handleKeyDown);
    }
  }, [showHotkeyDialog, listeningForKey]);

  useEffect(() => {
    const handleToggle = () => {
      if (showHotkeyDialog) {
        console.log("Toggle recording ignored: Hotkey dialog is open");
        return;
      }
      console.log("Toggle recording requested via hotkey/IPC");
      if (isRecording) {
        handleStopRecording();
      } else {
        handleStartRecording();
      }
    };
    window.addEventListener('toggle-recording', handleToggle);
    return () => window.removeEventListener('toggle-recording', handleToggle);
  }, [isRecording, currentVideo, showHotkeyDialog]);

  // Update handleStartRecording
  async function handleStartRecording() {
    if (isRecording) return;

    try {
      const monitors = await getMonitors();

      if (monitors.length > 1) {
        setShowMonitorSelect(true);
        return;
      }

      // If only one monitor, use it directly
      await startNewRecording('0');
    } catch (err) {
      console.error("Failed to handle start recording:", err);
      setError(err as string);
    }
  }

  // Update startNewRecording to handle string IDs
  async function startNewRecording(monitorId: string) {
    try {
      // Clear all states first
      setMousePositions([]);
      setIsVideoReady(false);
      setCurrentTime(0);
      setDuration(0);
      setIsPlaying(false);
      setSegment(null);
      setZoomFactor(1.5);
      setEditingKeyframeId(null);
      setThumbnails([]);

      // Clear previous video
      if (currentVideo) {
        URL.revokeObjectURL(currentVideo);
        setCurrentVideo(null);
      }
      if (currentAudio) {
        URL.revokeObjectURL(currentAudio);
        setCurrentAudio(null);
      }

      // Reset video element
      if (videoRef.current) {
        videoRef.current.pause();
        videoRef.current.src = "";
        videoRef.current.load();
        videoRef.current.removeAttribute('src');
        videoRef.current.currentTime = 0;
      }

      // Clear canvas
      const canvas = canvasRef.current;
      if (canvas) {
        const ctx = canvas.getContext('2d');
        if (ctx) {
          ctx.clearRect(0, 0, canvas.width, canvas.height);
        }
      }

      // Reset audio element
      if (audioRef.current) {
        audioRef.current.pause();
        audioRef.current.src = "";
        audioRef.current.load();
        audioRef.current.removeAttribute('src');
      }

      // Now start the new recording
      await invoke("start_recording", { monitorId });
      setIsRecording(true);
      setError(null);
    } catch (err) {
      console.error("Failed to start recording:", err);
      setError(err as string);
    }
  }

  // Update handleStopRecording
  async function handleStopRecording() {
    if (!isRecording) return;

    try {
      setIsRecording(false);
      setIsLoadingVideo(true);
      setIsVideoReady(false);
      setLoadingProgress(0);
      setThumbnails([]);

      // Capture audioPath from Rust
      const [videoUrl, audioUrl, rawMouseData, audioPath] = await invoke<[string, string, any[], string]>("stop_recording");

      setAudioFilePath(audioPath);

      // Explicitly map fields to handle potential camelCase vs snake_case mismatches
      const mouseData: MousePosition[] = rawMouseData.map(p => ({
        x: p.x,
        y: p.y,
        timestamp: p.timestamp,
        isClicked: p.isClicked !== undefined ? p.isClicked : p.is_clicked, // Handle both casing
        cursor_type: p.cursor_type || 'default'
      }));

      setMousePositions(mouseData);

      // Use the new centralized video loading
      const objectUrl = await videoControllerRef.current?.loadVideo({
        videoUrl,
        onLoadingProgress: (progress) => setLoadingProgress(progress)
      });

      if (objectUrl) {
        setCurrentVideo(objectUrl);

        // Load audio if available
        if (audioUrl) {
          const audioObjectUrl = await videoControllerRef.current?.loadAudio({
            audioUrl,
            onLoadingProgress: (p) => console.log('Audio Progress:', p)
          });
          if (audioObjectUrl) {
            setCurrentAudio(audioObjectUrl);
          }
        }

        setIsVideoReady(true);
        generateThumbnails();

        console.log(`[App] Received recording data. Video URL: ${videoUrl}, Audio URL: ${audioUrl}, Mouse Points: ${mouseData.length}, Clicks: ${mouseData.filter(p => p.isClicked).length}`);

        // Get actual video duration for the segment
        const videoDuration = videoRef.current?.duration || 0;
        const initialSegment: VideoSegment = {
          trimStart: 0,
          trimEnd: videoDuration,
          zoomKeyframes: [],
          textSegments: []
        };

        // Set segment immediately so first frame can render
        setSegment(initialSegment);

        // Render first frame immediately
        if (videoRef.current && canvasRef.current && videoRef.current.readyState >= 2) {
          videoRenderer.drawFrame({
            video: videoRef.current,
            canvas: canvasRef.current,
            tempCanvas: tempCanvasRef.current,
            segment: initialSegment,
            backgroundConfig,
            mousePositions: mouseData,
            currentTime: 0
          });
        }

        // Auto-save the initial project
        const response = await fetch(objectUrl);
        const videoBlob = await response.blob();
        const timestamp = new Date().toLocaleString();

        // Fetch audio blob if available
        let audioBlob: Blob | undefined;
        if (audioUrl) {
          const audioResponse = await fetch(audioUrl);
          audioBlob = await audioResponse.blob();
        }

        // Final frame render to ensure we have a thumbnail
        renderFrame();
        const thumbnail = generateThumbnail();

        const project = await projectManager.saveProject({
          name: `Recording ${timestamp}`,
          videoBlob,
          audioBlob,
          segment: initialSegment,
          backgroundConfig,
          mousePositions: mouseData,
          thumbnail
        });
        setCurrentProjectId(project.id);
        await loadProjects();
      }

    } catch (err) {
      setError(err as string);
    } finally {
      setIsLoadingVideo(false);
      setLoadingProgress(0);
    }
  }

  // Add cleanup for video object URL
  useEffect(() => {
    return () => {
      if (currentVideo && currentVideo.startsWith('blob:')) {
        URL.revokeObjectURL(currentVideo);
      }
    };
  }, [currentVideo]);

  // Add cleanup for audio object URL
  useEffect(() => {
    return () => {
      if (currentAudio && currentAudio.startsWith('blob:')) {
        URL.revokeObjectURL(currentAudio);
      }
    };
  }, [currentAudio]);

  // Toggle play/pause
  const togglePlayPause = () => {
    videoControllerRef.current?.togglePlayPause();
  };

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement;
      const isInput = ['INPUT', 'TEXTAREA'].includes(target.tagName);

      if (e.code === 'Space' && !isInput) {
        e.preventDefault();
        togglePlayPause();
      }

      // Delete Keyframe
      if ((e.code === 'Delete' || e.code === 'Backspace') && editingKeyframeId !== null && !isInput) {
        if (segment && segment.zoomKeyframes[editingKeyframeId]) {
          const newKeyframes = [...segment.zoomKeyframes];
          newKeyframes.splice(editingKeyframeId, 1);
          setSegment({ ...segment, zoomKeyframes: newKeyframes });
          setEditingKeyframeId(null);
        }
      }

      // Undo/Redo
      if (e.ctrlKey || e.metaKey) {
        if (e.code === 'KeyZ') {
          if (e.shiftKey) {
            e.preventDefault();
            if (canRedo) redo();
          } else {
            e.preventDefault();
            if (canUndo) undo();
          }
        } else if (e.code === 'KeyY') {
          e.preventDefault();
          if (canRedo) redo();
        }
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [togglePlayPause, editingKeyframeId, segment, canUndo, canRedo, undo, redo]);

  // Add this effect to handle metadata loading
  useEffect(() => {
    const video = videoRef.current;
    if (!video) return;

    const handleLoadedMetadata = () => {
      debugLog('Video loaded metadata', {
        duration: video.duration,
        width: video.videoWidth,
        height: video.videoHeight
      });

      if (video.duration !== Infinity) {
        setDuration(video.duration);
      }
    };

    const handleDurationChange = () => {
      debugLog('Duration changed:', video.duration);
      if (video.duration !== Infinity) {
        setDuration(video.duration);
      }
    };

    video.addEventListener('loadedmetadata', handleLoadedMetadata);
    video.addEventListener('durationchange', handleDurationChange);

    return () => {
      video.removeEventListener('loadedmetadata', handleLoadedMetadata);
      video.removeEventListener('durationchange', handleDurationChange);
    };
  }, []);

  // Replace the debugLog function
  const debugLog = (_message: string, _data?: any) => {
    // Disabled
  };

  // Add new export function to replace video-exporter.ts
  const handleExport = async () => {
    setShowExportDialog(true);
  };

  // Add new method to handle actual export
  const startExport = async () => {
    if (!currentVideo || !segment || !videoRef.current || !canvasRef.current) return;

    try {
      setShowExportDialog(false);
      setIsProcessing(true);

      // Create a complete export options object
      const exportConfig: ExportOptions & { audioFilePath: string } = {
        quality: exportOptions.quality,
        dimensions: exportOptions.dimensions,
        speed: exportOptions.speed,
        video: videoRef.current, // Passed as source reference only
        canvas: canvasRef.current,
        tempCanvas: tempCanvasRef.current,
        segment,
        backgroundConfig,
        mousePositions,
        audio: audioRef.current || undefined,
        audioFilePath: audioFilePath, // Pass the path here
        onProgress: (progress: number) => {
          setExportProgress(progress);
        }
      };

      await videoExporter.exportAndDownload(exportConfig);

    } catch (error) {
      console.error('[App] Export error:', error);
    } finally {
      setIsProcessing(false);
      setExportProgress(0);
    }
  };

  // Update handleAddKeyframe to include duration
  const handleAddKeyframe = (override?: Partial<ZoomKeyframe>) => {
    if (!segment || !videoRef.current) return;

    const currentTime = videoRef.current.currentTime;

    // Check for nearby keyframe to update (debounce/merge)
    const nearbyIndex = segment.zoomKeyframes.findIndex(k => Math.abs(k.time - currentTime) < 0.2);

    let updatedKeyframes: ZoomKeyframe[];

    if (nearbyIndex !== -1) {
      // Update existing keyframe
      const existing = segment.zoomKeyframes[nearbyIndex];
      updatedKeyframes = [...segment.zoomKeyframes];
      updatedKeyframes[nearbyIndex] = {
        ...existing,
        zoomFactor: override?.zoomFactor ?? existing.zoomFactor,
        positionX: override?.positionX ?? existing.positionX,
        positionY: override?.positionY ?? existing.positionY,
      };
      setEditingKeyframeId(nearbyIndex);
    } else {
      // Create new keyframe
      // Find previous keyframe for defaults
      const previousKeyframe = [...segment.zoomKeyframes]
        .sort((a, b) => b.time - a.time)
        .find(k => k.time < currentTime);

      const newKeyframe: ZoomKeyframe = {
        time: currentTime,
        duration: 1.0,
        zoomFactor: override?.zoomFactor ?? previousKeyframe?.zoomFactor ?? 1.5,
        positionX: override?.positionX ?? previousKeyframe?.positionX ?? 0.5,
        positionY: override?.positionY ?? previousKeyframe?.positionY ?? 0.5,
        easingType: 'easeInOut'
      };

      updatedKeyframes = [...segment.zoomKeyframes, newKeyframe]
        .sort((a, b) => a.time - b.time);

      setEditingKeyframeId(updatedKeyframes.indexOf(newKeyframe));
    }

    setSegment({
      ...segment,
      zoomKeyframes: updatedKeyframes
    });

    // Update zoomFactor state so the slider stays in sync during wheel/panning
    const finalFactor = override?.zoomFactor ?? updatedKeyframes[editingKeyframeId !== null ? nearbyIndex !== -1 ? nearbyIndex : updatedKeyframes.length - 1 : updatedKeyframes.length - 1]?.zoomFactor;
    if (finalFactor !== undefined) {
      setZoomFactor(finalFactor);
    }

    // Trigger AutoZoom update if smooth path is active
    if (segment.smoothMotionPath && segment.smoothMotionPath.length > 0) {
      // We need to re-run auto zoom generation with the new keyframes!
      // But we need the mouse data... which is in `mousePositions` state.
      // We need to access `mousePositions`.
      // BUT, generating path takes time.
      // We can trigger it in a useEffect or directly here if we have data.
    }
  };

  // Sync zoomFactor state with editing keyframe
  useEffect(() => {
    if (segment && editingKeyframeId !== null) {
      const kf = segment.zoomKeyframes[editingKeyframeId];
      if (kf) {
        setZoomFactor(kf.zoomFactor);
      }
    }
  }, [editingKeyframeId]);



  // Update the throttled update function for zoom configuration
  const throttledUpdateZoom = useThrottle((updates: Partial<ZoomKeyframe>) => {
    if (!segment || editingKeyframeId === null) return;

    const updatedKeyframes = segment.zoomKeyframes.map((keyframe, index) =>
      index === editingKeyframeId
        ? { ...keyframe, ...updates }
        : keyframe
    );

    setSegment({
      ...segment,
      zoomKeyframes: updatedKeyframes
    }, false);

    // Seek if needed (optional, usually dragging slider expects update)
    if (videoRef.current) {
      const kf = updatedKeyframes[editingKeyframeId];
      if (Math.abs(videoRef.current.currentTime - kf.time) > 0.1) {
        videoRef.current.currentTime = kf.time;
        setCurrentTime(kf.time);
      }
    }

    // Force a redraw to show the changes
    requestAnimationFrame(() => {
      renderFrame();
    });
  }, 32); // 32ms throttle

  // Non-passive wheel listener to fix scrolling issue
  useEffect(() => {
    const container = previewContainerRef.current;
    if (!container) return;

    const handleWheel = (e: WheelEvent) => {
      if (!currentVideo || isCropping) return;
      e.preventDefault();
      e.stopPropagation();

      const lastState = videoRenderer.getLastCalculatedState();
      if (!lastState) return;

      // Sensitivity
      const zoomDelta = -e.deltaY * 0.002 * lastState.zoomFactor;
      const newZoom = Math.max(1.0, Math.min(12.0, lastState.zoomFactor + zoomDelta));

      handleAddKeyframe({
        zoomFactor: newZoom,
        positionX: lastState.positionX,
        positionY: lastState.positionY
      });
      setActivePanel('zoom');
    };

    container.addEventListener('wheel', handleWheel, { passive: false });
    return () => container.removeEventListener('wheel', handleWheel);
  }, [currentVideo, segment]); // Re-bind if segment changes? No, handleAddKeyframe uses ref state mostly but logic is closed over? 
  // handleAddKeyframe in App depends on 'segment'.
  // If 'segment' changes, handleAddKeyframe is stale?
  // Yes, functions in App are re-created.
  // We need to fetch fresh state or use ref for handlers.
  // Actually, 'handleAddKeyframe' is stable dependency? No it changes on render.
  // So we must include it in dep array or `handleWheel` calls old closure.
  // Added [handleAddKeyframe] to dependencies.

  // Add this effect to redraw when background config changes
  useEffect(() => {
    if (videoRef.current && !videoRef.current.paused) return; // Don't interrupt if playing

    // Create a proper FrameRequestCallback
    const frameCallback: FrameRequestCallback = (_time: number) => {
      renderFrame();
    };

    requestAnimationFrame(frameCallback);
  }, [backgroundConfig, renderFrame]);

  // Add this state near the top of the App component
  const [recordingDuration, setRecordingDuration] = useState(0);

  // Add this effect to track recording duration
  useEffect(() => {
    let interval: number;

    if (isRecording) {
      const startTime = Date.now();
      interval = window.setInterval(() => {
        setRecordingDuration(Math.floor((Date.now() - startTime) / 1000));
      }, 1000);
    } else {
      setRecordingDuration(0);
    }

    return () => {
      if (interval) {
        clearInterval(interval);
      }
    };
  }, [isRecording]);

  // Add this effect after the other useEffect hooks
  useEffect(() => {
    if (!segment || !isVideoReady) return;

    // Find the active keyframe based on current time
    const findActiveKeyframe = () => {
      const sortedKeyframes = [...segment.zoomKeyframes].sort((a, b) => a.time - b.time);

      for (let i = 0; i < sortedKeyframes.length; i++) {
        // Use the helper to compute rangeStart and rangeEnd
        const { rangeStart, rangeEnd } = getKeyframeRange(sortedKeyframes, i);

        // Check if current time is within this keyframe's range
        if (currentTime >= rangeStart && currentTime <= rangeEnd) {
          if (editingKeyframeId !== i) {
            setEditingKeyframeId(i);
            setZoomFactor(sortedKeyframes[i].zoomFactor);
            if (activePanel !== "zoom") {
              setActivePanel("zoom");
            }
          }
          return;
        }
      }

      // If we're not in any keyframe's range, deselect
      if (editingKeyframeId !== null) {
        setEditingKeyframeId(null);
      }
    };

    findActiveKeyframe();
  }, [currentTime, segment, isVideoReady]);

  // Update the loading placeholder to show progress
  const renderPlaceholder = () => {
    return (
      <div className="absolute inset-0 bg-[#1a1a1b] flex flex-col items-center justify-center">
        {/* Grid pattern background */}
        <div className="absolute inset-0 opacity-5">
          <div className="w-full h-full" style={{
            backgroundImage: `
              linear-gradient(to right, #fff 1px, transparent 1px),
              linear-gradient(to bottom, #fff 1px, transparent 1px)
            `,
            backgroundSize: '20px 20px'
          }} />
        </div>

        {isLoadingVideo ? (
          // Loading state after recording
          <div className="flex flex-col items-center">
            <Loader2 className="w-12 h-12 text-[#0079d3] animate-spin mb-4" />
            <p className="text-[#d7dadc] font-medium">Processing Video</p>
            <p className="text-[#818384] text-sm mt-1">This may take a few moments...</p>
          </div>
        ) : isRecording ? (
          // Recording state (only show if no video is loaded)
          <div className="flex flex-col items-center">
            <div className="w-4 h-4 rounded-full bg-red-500 animate-pulse mb-4" />
            <p className="text-[#d7dadc] font-medium">Recording in progress...</p>
            <p className="text-[#818384] text-sm mt-1">Screen is being captured</p>
            <span className="text-[#d7dadc] text-xl font-mono mt-4">{formatTime(recordingDuration)}</span>
          </div>
        ) : (
          // No video state
          <div className="flex flex-col items-center">
            <Video className="w-12 h-12 text-[#343536] mb-4" />
            <p className="text-[#d7dadc] font-medium">No Video Selected</p>
            <p className="text-[#818384] text-sm mt-1">Click 'Start Recording' to begin</p>
          </div>
        )}
        {isLoadingVideo && loadingProgress > 0 && (
          <div className="mt-2">
            <p className="text-[#818384] text-sm">
              Loading video: {Math.min(Math.round(loadingProgress), 100)}%
            </p>
          </div>
        )}
      </div>
    );
  };

  // Add new state for export options
  const [showExportDialog, setShowExportDialog] = useState(false);
  const [exportOptions, setExportOptions] = useState<ExportOptions>({
    quality: 'balanced',
    dimensions: '1080p',
    speed: 1 // Default to 100% speed
  });

  // Add these state variables in the App component
  const [projects, setProjects] = useState<Omit<Project, 'videoBlob'>[]>([]);
  const [showProjectsDialog, setShowProjectsDialog] = useState(false);

  // Add this effect to load projects on mount
  useEffect(() => {
    loadProjects();
  }, []);

  // Add these functions to the App component
  const loadProjects = async () => {
    const projects = await projectManager.getProjects();
    setProjects(projects);
  };

  // States removed: showSaveDialog, projectNameInput
  const [currentProjectId, setCurrentProjectId] = useState<string | null>(null);
  const [editingProjectNameId, setEditingProjectNameId] = useState<string | null>(null);
  const [projectRenameValue, setProjectRenameValue] = useState("");

  const generateThumbnail = useCallback((): string | undefined => {
    if (!canvasRef.current) return undefined;
    try {
      return canvasRef.current.toDataURL('image/jpeg', 0.5);
    } catch (e) {
      return undefined;
    }
  }, []);

  // Update handleSaveProject to show different options when editing existing project
  // Auto-save effect
  useEffect(() => {
    if (!currentProjectId || !currentVideo || !segment) return;

    const performAutoSave = async () => {
      try {
        const response = await fetch(currentVideo);
        const videoBlob = await response.blob();
        const thumbnail = generateThumbnail();
        await projectManager.updateProject(currentProjectId, {
          name: projects.find(p => p.id === currentProjectId)?.name || "Auto Saved Project",
          videoBlob,
          segment,
          backgroundConfig,
          mousePositions,
          thumbnail
        });
        await loadProjects();
      } catch (err) {
        // Silent fail on auto-save
      }
    };

    const timer = setTimeout(performAutoSave, 2000);
    return () => clearTimeout(timer);
  }, [segment, backgroundConfig, mousePositions, currentProjectId]);

  const handleRenameProject = async (id: string) => {
    if (!projectRenameValue.trim()) return;
    const project = projects.find(p => p.id === id);
    if (!project) return;

    // Load full project to update name
    const fullProject = await projectManager.loadProject(id);
    if (fullProject) {
      await projectManager.updateProject(id, {
        ...fullProject,
        name: projectRenameValue.trim()
      });
      await loadProjects();
    }
    setEditingProjectNameId(null);
  };

  // Update handleLoadProject to use loadVideo instead of handleVideoSourceChange
  const handleLoadProject = async (projectId: string) => {
    const project = await projectManager.loadProject(projectId);
    if (!project) return;

    // Clear previous video and audio URLs
    if (currentVideo) URL.revokeObjectURL(currentVideo);
    if (currentAudio) URL.revokeObjectURL(currentAudio);

    setThumbnails([]);
    setCurrentAudio(null);

    // Load Video
    console.log('[App] Loading project video blob:', project.videoBlob.size);
    const videoObjectUrl = await videoControllerRef.current?.loadVideo({ videoBlob: project.videoBlob });
    if (videoObjectUrl) setCurrentVideo(videoObjectUrl);

    // Load Audio
    if (project.audioBlob) {
      console.log('[App] Loading project audio blob:', project.audioBlob.size);
      const audioObjectUrl = await videoControllerRef.current?.loadAudio({ audioBlob: project.audioBlob });
      if (audioObjectUrl) setCurrentAudio(audioObjectUrl);
    } else {
      console.log('[App] Project has no audio blob');
      setCurrentAudio(null);
    }

    // Fix trimEnd: 0 from saved projects - use actual video duration
    const videoDuration = videoControllerRef.current?.duration || 0;
    const correctedSegment = { ...project.segment };
    if (correctedSegment.trimEnd === 0 || correctedSegment.trimEnd > videoDuration) {
      correctedSegment.trimEnd = videoDuration;
    }


    setSegment(correctedSegment);
    setBackgroundConfig(project.backgroundConfig);
    setMousePositions(project.mousePositions);

    // Sync volume immediately
    if (videoControllerRef.current && project.backgroundConfig.volume !== undefined) {
      videoControllerRef.current.setVolume(project.backgroundConfig.volume);
    }

    setShowProjectsDialog(false);
    setCurrentProjectId(projectId);
  };

  // Add these states in App component
  const [thumbnails, setThumbnails] = useState<string[]>([]);

  // Replace the existing generateThumbnails function
  const generateThumbnails = useCallback(async () => {
    if (!currentVideo || !segment) return;

    const thumbnails = await thumbnailGenerator.generateThumbnails(currentVideo, 20, {
      trimStart: segment.trimStart,
      trimEnd: segment.trimEnd
    });

    setThumbnails(thumbnails);
  }, [currentVideo, segment]);

  // Add this effect
  useEffect(() => {
    if (isVideoReady && duration > 0 && thumbnails.length === 0) {
      generateThumbnails();
    }
  }, [isVideoReady, duration, generateThumbnails]);

  // Add this state near the top of App component
  const [recentUploads, setRecentUploads] = useState<string[]>([]);

  // Update handleBackgroundUpload function
  const handleBackgroundUpload = (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (file) {
      const reader = new FileReader();
      reader.onload = (event) => {
        const imageUrl = event.target?.result as string;
        // Update background config
        setBackgroundConfig(prev => ({
          ...prev,
          backgroundType: 'custom',
          customBackground: imageUrl
        }));

        // Update recent uploads (keep last 3)
        setRecentUploads(prev => {
          const newUploads = [imageUrl, ...prev].slice(0, 3);
          return newUploads;
        });
      };
      reader.readAsDataURL(file);
    }
  };

  // Initialize segment when video loads
  useEffect(() => {
    if (duration > 0 && !segment) {
      const initialSegment: VideoSegment = {
        trimStart: 0,
        trimEnd: duration,
        zoomKeyframes: [],
        textSegments: []
      };
      setSegment(initialSegment);

      // Render first frame after segment is set (next tick to ensure state is updated)
      setTimeout(() => {
        if (videoRef.current && canvasRef.current && videoRef.current.readyState >= 2) {
          videoRenderer.drawFrame({
            video: videoRef.current,
            canvas: canvasRef.current,
            tempCanvas: tempCanvasRef.current,
            segment: initialSegment,
            backgroundConfig,
            mousePositions,
            currentTime: 0
          });
        }
      }, 0);
    }
  }, [duration, segment, backgroundConfig, mousePositions]);

  // Add this state for text segments
  const [editingTextId, setEditingTextId] = useState<string | null>(null);

  // Add this function to handle adding new text segments
  const handleAddText = () => {
    if (!segment) return;

    const newText: TextSegment = {
      id: crypto.randomUUID(),
      startTime: currentTime,
      endTime: Math.min(currentTime + 3, duration),
      text: 'New Text',
      style: {
        fontSize: 24,
        color: '#ffffff',
        x: 50,  // Center by default
        y: 50   // Center by default
      }
    };

    setSegment({
      ...segment,
      textSegments: [...(segment.textSegments || []), newText]
    });
    setEditingTextId(newText.id);
    setActivePanel('text');
  };

  // Add these handlers in the App component
  const handleTextDragMove = (id: string, x: number, y: number) => {
    if (!segment) return;
    setSegment({
      ...segment,
      textSegments: segment.textSegments.map(t =>
        t.id === id ? { ...t, style: { ...t.style, x, y } } : t
      )
    });
  };

  // Add event listeners to the canvas
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas || !segment) return;

    const handleMouseDown = (e: MouseEvent) => {
      videoRenderer.handleMouseDown(e, segment, canvas);
    };

    const handleMouseMove = (e: MouseEvent) => {
      videoRenderer.handleMouseMove(e, segment, canvas, handleTextDragMove);
    };

    const handleMouseUp = () => {
      videoRenderer.handleMouseUp(canvas);
    };

    canvas.addEventListener('mousedown', handleMouseDown);
    canvas.addEventListener('mousemove', handleMouseMove);
    canvas.addEventListener('mouseup', handleMouseUp);
    canvas.addEventListener('mouseleave', handleMouseUp);

    return () => {
      canvas.removeEventListener('mousedown', handleMouseDown);
      canvas.removeEventListener('mousemove', handleMouseMove);
      canvas.removeEventListener('mouseup', handleMouseUp);
      canvas.removeEventListener('mouseleave', handleMouseUp);
    };
  }, [segment]);

  return (
    <div className="min-h-screen bg-[#1a1a1b]">
      <header
        className="bg-[#1a1a1b] border-b border-[#343536] select-none h-11 flex items-center justify-between cursor-default"
        onMouseDown={() => {
          (window as any).ipc.postMessage('drag_window');
        }}
      >
        <div className="flex items-center gap-4 px-4 h-full">
          <div className="flex items-center gap-3">
            <Video className="w-5 h-5 text-[#0079d3]" />
            <span className="text-[#d7dadc] text-sm font-medium">Screen Record</span>
          </div>

          <div className="h-full flex items-center">
            {isRecording && currentVideo && (
              <div className="flex items-center gap-3 bg-red-500/10 border border-red-500/30 px-3 py-1 rounded-full animate-in fade-in slide-in-from-left-2 duration-300">
                <div className="w-2 h-2 rounded-full bg-red-500 animate-pulse" />
                <div className="flex flex-col">
                  <span className="text-red-500 text-[10px] font-bold leading-none uppercase tracking-wider">Recording</span>
                  <span className="text-[#818384] text-[9px] leading-tight">Screen is being captured</span>
                </div>
                <span className="text-[#d7dadc] text-xs font-mono ml-1">{formatTime(recordingDuration)}</span>
              </div>
            )}
          </div>
        </div>

        <div className="flex items-center gap-3 h-full px-2">
          <div className="flex items-center gap-2 flex-wrap max-w-[400px] justify-end">
            {hotkeys.map((h, i) => (
              <Button
                key={i}
                onMouseDown={(e) => e.stopPropagation()}
                onClick={() => handleRemoveHotkey(i)}
                className="bg-[#272729] hover:bg-red-500/20 text-[#d7dadc] hover:text-red-400 px-2 h-7 text-xs border border-transparent hover:border-red-500/30 flex-shrink-0"
                title="Click to remove"
              >
                <span className="truncate max-w-[80px]">{h.name}</span>
                <X className="w-3 h-3 ml-1 flex-shrink-0" />
              </Button>
            ))}
            <Button
              onMouseDown={(e) => e.stopPropagation()}
              onClick={() => { setShowHotkeyDialog(true); setListeningForKey(true); }}
              className="bg-[#0079d3] hover:bg-[#0079d3]/90 text-white px-2 h-7 text-xs flex-shrink-0"
              title="Add Global Hotkey"
            >
              <Keyboard className="w-3 h-3 mr-1" />
              Add Hotkey
            </Button>
            <Button
              onMouseDown={(e) => e.stopPropagation()}
              onClick={toggleKeyviz}
              className={`px-2 h-7 text-xs flex-shrink-0 transition-colors ${keyvizStatus.enabled
                ? 'bg-green-600 hover:bg-green-700 text-white'
                : 'bg-[#272729] hover:bg-[#343536] text-[#d7dadc]'
                }`}
              title={keyvizStatus.installed ? "Toggle Keyviz" : "Install & Enable Keyviz"}
            >
              <Keyboard className="w-3 h-3 mr-1" />
              {keyvizStatus.enabled ? "Keystrokes: ON" : "Show Keystrokes"}
            </Button>
          </div>

          <div className="flex items-center gap-2">
            {currentVideo && (
              <Button
                onMouseDown={(e) => e.stopPropagation()}
                onClick={handleExport}
                disabled={isProcessing}
                className={`flex items-center px-4 py-2 h-8 text-xs font-medium ${isProcessing
                  ? 'bg-gray-600 text-gray-400 cursor-not-allowed'
                  : 'bg-[#9C17FF] hover:bg-[#9C17FF]/90 text-white'
                  }`}
              >
                <Download className="w-4 h-4 mr-2" />Export
              </Button>
            )}
            <Button
              variant="ghost"
              size="sm"
              onMouseDown={(e) => e.stopPropagation()}
              onClick={() => setShowProjectsDialog(true)}
              className="h-8 text-xs text-[#d7dadc] hover:bg-[#272729]"
            >
              <FolderOpen className="w-4 h-4 mr-2" />Projects
            </Button>
          </div>

          <div className="flex items-center h-full ml-4">
            <button
              onMouseDown={(e) => e.stopPropagation()}
              onClick={(e) => {
                e.stopPropagation();
                (window as any).ipc.postMessage('minimize_window');
              }}
              className="px-3 h-full text-[#d7dadc] hover:bg-[#272729] transition-colors flex items-center"
              title="Minimize"
            >
              <Minus className="w-4 h-4" />
            </button>
            <button
              onMouseDown={(e) => e.stopPropagation()}
              onClick={async (e) => {
                e.stopPropagation();
                (window as any).ipc.postMessage('toggle_maximize');
                // Small delay to let the state settle before checking
                setTimeout(async () => {
                  const maximized = await invoke<boolean>('is_maximized');
                  setIsWindowMaximized(maximized);
                }, 50);
              }}
              className="px-3 h-full text-[#d7dadc] hover:bg-[#272729] transition-colors flex items-center"
              title={isWindowMaximized ? "Restore" : "Maximize"}
            >
              {isWindowMaximized ? <Copy className="w-3.5 h-3.5" /> : <Square className="w-3.5 h-3.5" />}
            </button>
            <button
              onMouseDown={(e) => e.stopPropagation()}
              onClick={(e) => {
                e.stopPropagation();
                (window as any).ipc.postMessage('close_window');
              }}
              className="px-3 h-full text-[#d7dadc] hover:bg-[#e81123] hover:text-white transition-colors flex items-center"
              title="Close"
            >
              <X className="w-4 h-4" />
            </button>
          </div>
        </div>
      </header>

      <main className="max-w-6xl mx-auto px-4 py-6">
        {error && <p className="text-red-500 mb-4">{error}</p>}

        <div className="space-y-6">
          <div className="grid grid-cols-4 gap-6 items-start">
            <div className="col-span-3 rounded-lg overflow-hidden bg-black/20 flex items-center justify-center">
              <div className="relative w-full flex justify-center max-h-[70vh]">
                <div
                  ref={previewContainerRef}
                  className={`relative flex items-center justify-center cursor-crosshair group ${!currentVideo ? 'w-full aspect-video' : ''}`}
                  onMouseDown={(e) => {
                    if (!currentVideo) return;
                    if (isCropping) return; // Disable pan/zoom when cropping
                    if (activePanel === 'text') return; // Disable panning when in text mode (allow text drag)
                    e.preventDefault();
                    e.stopPropagation(); // Prevent drag of window if any

                    if (isPlaying) togglePlayPause();

                    const startX = e.clientX;
                    const startY = e.clientY;
                    const lastState = videoRenderer.getLastCalculatedState();
                    if (!lastState) return;

                    const startPosX = lastState.positionX;
                    const startPosY = lastState.positionY;
                    const z = lastState.zoomFactor;
                    const rect = e.currentTarget.getBoundingClientRect();

                    const handleMouseMove = (me: MouseEvent) => {
                      const dx = me.clientX - startX;
                      const dy = me.clientY - startY;

                      // Drag World: Dragging Right (dx > 0) moves camera Left (pos decreases)
                      const ndx = -(dx / rect.width) / z;
                      const ndy = -(dy / rect.height) / z;

                      handleAddKeyframe({
                        zoomFactor: z,
                        positionX: Math.max(0, Math.min(1, startPosX + ndx)),
                        positionY: Math.max(0, Math.min(1, startPosY + ndy))
                      });
                      setActivePanel('zoom');
                    };

                    const handleMouseUp = () => {
                      window.removeEventListener('mousemove', handleMouseMove);
                      window.removeEventListener('mouseup', handleMouseUp);
                    };

                    window.addEventListener('mousemove', handleMouseMove);
                    window.addEventListener('mouseup', handleMouseUp);
                  }}
                >
                  <canvas
                    ref={canvasRef}
                    className="max-w-full max-h-[70vh] object-contain"
                  />
                  <canvas ref={tempCanvasRef} className="hidden" />
                  <video ref={videoRef} className="hidden" playsInline preload="auto" />
                  <audio ref={audioRef} className="hidden" />
                  {(!currentVideo || isLoadingVideo) && renderPlaceholder()}

                  {/* Crop Overlay */}
                  {isCropping && currentVideo && segment && (
                    <div
                      className="absolute inset-0 z-20 pointer-events-none"
                    >
                      {(() => {
                        // 1. Calculate the actual video rectangle within the container
                        const container = previewContainerRef.current;
                        const video = videoRef.current;
                        // We need to ensure we can read dimensions. If video isn't ready, we can't show crop box correctly aligned.
                        if (!container || !video) return null;

                        const containerRect = container.getBoundingClientRect();
                        const vidW = video.videoWidth;
                        const vidH = video.videoHeight;

                        if (!vidW || !vidH) return null;

                        const containerRatio = containerRect.width / containerRect.height;
                        const videoRatio = vidW / vidH;

                        let renderW, renderH, renderTop, renderLeft;

                        if (containerRatio > videoRatio) {
                          // Container is wider -> Video fits height
                          renderH = containerRect.height;
                          renderW = renderH * videoRatio;
                          renderTop = 0;
                          renderLeft = (containerRect.width - renderW) / 2;
                        } else {
                          // Container is taller -> Video fits width
                          renderW = containerRect.width;
                          renderH = renderW / videoRatio;
                          renderLeft = 0;
                          renderTop = (containerRect.height - renderH) / 2;
                        }

                        // 2. Wrap the crop logic in a div positioned exactly over the video content
                        return (
                          <div
                            style={{
                              position: 'absolute',
                              left: renderLeft,
                              top: renderTop,
                              width: renderW,
                              height: renderH,
                            }}
                          >
                            {(() => {
                              // Current Crop State
                              // Default to full video if undefined
                              const crop = segment.crop || { x: 0, y: 0, width: 1, height: 1 };

                              // Visual properties (in %)
                              const leftPct = crop.x;
                              const topPct = crop.y;
                              const widthPct = crop.width;
                              const heightPct = crop.height;

                              // Helper for resize handlers
                              const handleResizeStart = (e: React.MouseEvent, type: string) => {
                                e.preventDefault();
                                e.stopPropagation();
                                const startX = e.clientX;
                                const startY = e.clientY;
                                const startCrop = { ...crop };

                                const rectW = renderW;
                                const rectH = renderH;

                                const handleMove = (me: MouseEvent) => {
                                  const dx = (me.clientX - startX);
                                  const dy = (me.clientY - startY);

                                  // Convert to percentage change
                                  const dXPct = dx / rectW;
                                  const dYPct = dy / rectH;

                                  let newX = startCrop.x;
                                  let newY = startCrop.y;
                                  let newW = startCrop.width;
                                  let newH = startCrop.height;

                                  // Adjust based on handle type
                                  // N/S affect Y and Height
                                  if (type.includes('n')) {
                                    // If dragging North: New Top = Old Top + Delta
                                    // New Height = Old Height - Delta
                                    // Constraint: New Top >= 0, New Height >= Min
                                    let desiredY = startCrop.y + dYPct;
                                    // Clamp Y between 0 and (Bottom - MinHeight)
                                    const maxY = startCrop.y + startCrop.height - 0.05;
                                    desiredY = Math.max(0, Math.min(maxY, desiredY));

                                    const deltaY = desiredY - startCrop.y;
                                    newY = desiredY;
                                    newH = startCrop.height - deltaY;
                                  } else if (type.includes('s')) {
                                    // Dragging South: Top fixed, Height changes
                                    // New Height = Old Height + Delta
                                    let desiredH = startCrop.height + dYPct;
                                    // Clamp H: Min <= H <= (1 - Top)
                                    newH = Math.max(0.05, Math.min(1 - startCrop.y, desiredH));
                                  }

                                  // E/W affect X and Width
                                  if (type.includes('w')) {
                                    // Dragging West: New Left = Old Left + Delta
                                    // New Width = Old Width - Delta
                                    let desiredX = startCrop.x + dXPct;
                                    const maxX = startCrop.x + startCrop.width - 0.05;
                                    desiredX = Math.max(0, Math.min(maxX, desiredX));

                                    const deltaX = desiredX - startCrop.x;
                                    newX = desiredX;
                                    newW = startCrop.width - deltaX;
                                  } else if (type.includes('e')) {
                                    // Dragging East
                                    let desiredW = startCrop.width + dXPct;
                                    newW = Math.max(0.05, Math.min(1 - startCrop.x, desiredW));
                                  }

                                  // Update Segment State
                                  setSegment(prev => prev ? ({
                                    ...prev,
                                    crop: { x: newX, y: newY, width: newW, height: newH }
                                  }) : null);
                                };

                                const handleUp = () => {
                                  window.removeEventListener('mousemove', handleMove);
                                  window.removeEventListener('mouseup', handleUp);
                                };
                                window.addEventListener('mousemove', handleMove);
                                window.addEventListener('mouseup', handleUp);
                              };

                              return (
                                <div
                                  className="absolute border-2 border-[#0079d3] bg-[#0079d3]/10 pointer-events-auto"
                                  style={{
                                    left: `${leftPct * 100}%`,
                                    top: `${topPct * 100}%`,
                                    width: `${widthPct * 100}%`,
                                    height: `${heightPct * 100}%`,
                                    boxShadow: '0 0 0 9999px rgba(0, 0, 0, 0.7)'
                                  }}
                                  onMouseDown={(e) => {
                                    e.preventDefault();
                                    e.stopPropagation();
                                    const startX = e.clientX;
                                    const startY = e.clientY;
                                    const startCrop = { ...crop };
                                    const rectW = renderW;
                                    const rectH = renderH;

                                    const handleBoxMove = (me: MouseEvent) => {
                                      const dx = (me.clientX - startX) / rectW;
                                      const dy = (me.clientY - startY) / rectH;

                                      // Move crop rect, clamping to bounds
                                      let newX = startCrop.x + dx;
                                      let newY = startCrop.y + dy;

                                      // Clamp
                                      newX = Math.max(0, Math.min(1 - startCrop.width, newX));
                                      newY = Math.max(0, Math.min(1 - startCrop.height, newY));

                                      setSegment(prev => prev ? ({
                                        ...prev,
                                        crop: { x: newX, y: newY, width: startCrop.width, height: startCrop.height }
                                      }) : null);
                                    };
                                    const handleUp = () => {
                                      window.removeEventListener('mousemove', handleBoxMove);
                                      window.removeEventListener('mouseup', handleUp);
                                    };
                                    window.addEventListener('mousemove', handleBoxMove);
                                    window.addEventListener('mouseup', handleUp);
                                  }}
                                >
                                  {/* Grid Lines (Rule of Thirds) */}
                                  <div className="absolute inset-0 flex flex-col pointer-events-none opacity-30">
                                    <div className="flex-1 border-b border-white/50" />
                                    <div className="flex-1 border-b border-white/50" />
                                    <div className="flex-1" />
                                  </div>
                                  <div className="absolute inset-0 flex pointer-events-none opacity-30">
                                    <div className="flex-1 border-r border-white/50" />
                                    <div className="flex-1 border-r border-white/50" />
                                    <div className="flex-1" />
                                  </div>

                                  {/* 8 Handles */}
                                  {[
                                    { t: 'nw', c: 'cursor-nw-resize', s: '-top-1.5 -left-1.5' },
                                    { t: 'n', c: 'cursor-n-resize', s: '-top-1.5 left-1/2 -translate-x-1/2' },
                                    { t: 'ne', c: 'cursor-ne-resize', s: '-top-1.5 -right-1.5' },
                                    { t: 'w', c: 'cursor-w-resize', s: 'top-1/2 -translate-y-1/2 -left-1.5' },
                                    { t: 'e', c: 'cursor-e-resize', s: 'top-1/2 -translate-y-1/2 -right-1.5' },
                                    { t: 'sw', c: 'cursor-sw-resize', s: '-bottom-1.5 -left-1.5' },
                                    { t: 's', c: 'cursor-s-resize', s: '-bottom-1.5 left-1/2 -translate-x-1/2' },
                                    { t: 'se', c: 'cursor-se-resize', s: '-bottom-1.5 -right-1.5' },
                                  ].map(handle => (
                                    <div
                                      key={handle.t}
                                      className={`absolute w-3 h-3 bg-white border border-[#0079d3] rounded-full z-30 hover:scale-125 transition-transform ${handle.c} ${handle.s}`}
                                      onMouseDown={(e) => handleResizeStart(e, handle.t)}
                                    />
                                  ))}

                                  {/* Central Crosshair */}
                                  <div className="absolute top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 w-4 h-4 opacity-50 pointer-events-none">
                                    <div className="absolute w-full h-[1px] bg-white top-1/2 -translate-y-1/2 shadow-sm" />
                                    <div className="absolute h-full w-[1px] bg-white left-1/2 -translate-x-1/2 shadow-sm" />
                                  </div>
                                </div>
                              );
                            })()}
                          </div>
                        );
                      })()}
                    </div>
                  )}

                </div>
                {currentVideo && !isLoadingVideo && (
                  <div className="absolute bottom-4 left-1/2 transform -translate-x-1/2 flex items-center gap-3 bg-black/80 rounded-full px-4 py-2 backdrop-blur-sm z-50">
                    <Button
                      onClick={() => {
                        if (isCropping) {
                          // Confirm/Apply Crop
                          setIsCropping(false);
                          setActivePanel('zoom');
                          // Reset Zoom to 1.0 to view the new crop clearly without inherited zoom
                          setZoomFactor(1.0);
                          // Also reset any focused keyframe to avoid confusion
                          setEditingKeyframeId(null);
                        } else {
                          // Enter Crop Mode
                          setIsCropping(true);
                          if (isPlaying) togglePlayPause();
                        }
                      }}
                      variant="ghost"
                      size="icon"
                      className={`w-8 h-8 rounded-full transition-colors ${isCropping ? 'bg-green-500/80 text-white hover:bg-green-600' : 'text-white/80 hover:text-white hover:bg-white/10'}`}
                      title={isCropping ? "Apply Crop" : "Crop Video"}
                    >
                      {isCropping ? (
                        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><polyline points="20 6 9 17 4 12" /></svg>
                      ) : (
                        <Crop className="w-4 h-4" />
                      )}
                    </Button>
                    <div className="w-px h-4 bg-white/20 mx-1" />
                    <Button
                      onClick={togglePlayPause}
                      disabled={isProcessing || !isVideoReady}
                      variant="ghost"
                      size="icon"
                      className={`w-8 h-8 rounded-full transition-colors text-white bg-transparent hover:text-white hover:bg-transparent ${isProcessing || !isVideoReady
                        ? 'opacity-50 cursor-not-allowed'
                        : ''
                        }`}
                    >
                      {isPlaying ? (
                        <Pause className="w-4 h-4" />
                      ) : (
                        <Play className="w-4 h-4 ml-0.5" />
                      )}
                    </Button>
                    <div className="text-white/90 text-sm font-medium">
                      {formatTime(currentTime)} / {formatTime(duration)}
                    </div>
                  </div>
                )}
              </div>
            </div>

            <div className="col-span-1 space-y-3">
              <div className="flex bg-[#272729] p-0.5 rounded-md">
                <Button
                  onClick={() => setActivePanel('zoom')}
                  variant={activePanel === 'zoom' ? 'default' : 'outline'}
                  size="sm"
                  className={`flex-1 ${activePanel === 'zoom'
                    ? 'bg-[#1a1a1b] text-[#d7dadc] border-0'
                    : 'bg-transparent text-[#818384] border-0 hover:bg-[#1a1a1b]/10 hover:text-[#d7dadc]'
                    }`}
                >
                  Zoom
                </Button>
                <Button
                  onClick={() => setActivePanel('background')}
                  variant={activePanel === 'background' ? 'default' : 'outline'}
                  size="sm"
                  className={`flex-1 ${activePanel === 'background'
                    ? 'bg-[#1a1a1b] text-[#d7dadc] border-0'
                    : 'bg-transparent text-[#818384] border-0 hover:bg-[#1a1a1b]/10 hover:text-[#d7dadc]'
                    }`}
                >
                  Background
                </Button>
                <Button
                  onClick={() => setActivePanel('cursor')}
                  variant={activePanel === 'cursor' ? 'default' : 'outline'}
                  size="sm"
                  className={`flex-1 ${activePanel === 'cursor'
                    ? 'bg-[#1a1a1b] text-[#d7dadc] border-0'
                    : 'bg-transparent text-[#818384] border-0 hover:bg-[#1a1a1b]/10 hover:text-[#d7dadc]'
                    }`}
                >
                  Cursor
                </Button>
                <Button
                  onClick={() => setActivePanel('text')}
                  variant={activePanel === 'text' ? 'default' : 'outline'}
                  size="sm"
                  className={`flex-1 ${activePanel === 'text'
                    ? 'bg-[#1a1a1b] text-[#d7dadc] border-0'
                    : 'bg-transparent text-[#818384] border-0 hover:bg-[#1a1a1b]/10 hover:text-[#d7dadc]'
                    }`}
                >
                  Text
                </Button>
              </div>

              {activePanel === 'zoom' ? (
                <>
                  {(editingKeyframeId !== null) ? (
                    <div className="bg-[#1a1a1b] rounded-lg border border-[#343536] p-4">
                      <div className="flex justify-between items-center mb-4">
                        <h2 className="text-base font-semibold text-[#d7dadc]">Zoom Configuration</h2>
                        {editingKeyframeId !== null && <Button onClick={() => { if (segment && editingKeyframeId !== null) { setSegment({ ...segment, zoomKeyframes: segment.zoomKeyframes.filter((_, i) => i !== editingKeyframeId) }); setEditingKeyframeId(null); } }} variant="ghost" size="icon" className="text-[#d7dadc] hover:text-red-400 hover:bg-red-400/10 transition-colors"><Trash2 className="w-5 h-5" /></Button>}
                      </div>
                      <div className="space-y-4">
                        <div>
                          <label className="text-sm font-medium text-[#d7dadc] mb-2">Zoom Factor</label>
                          <div className="space-y-2">
                            <input type="range" min="1" max="3" step="0.1" value={zoomFactor} onChange={(e) => { const newValue = Number(e.target.value); setZoomFactor(newValue); throttledUpdateZoom({ zoomFactor: newValue }); }} className="w-full accent-[#0079d3]" />
                            <div className="flex justify-between text-xs text-[#818384] font-medium">
                              <span>1x</span>
                              <span>{zoomFactor.toFixed(1)}x</span>
                              <span>3x</span>
                            </div>
                          </div>
                        </div>
                        <div className="space-y-4">
                          <div>
                            <label className="text-sm font-medium text-[#d7dadc] mb-2 flex justify-between"><span>Horizontal Position</span><span className="text-[#818384]">{Math.round((segment?.zoomKeyframes[editingKeyframeId!]?.positionX ?? 0.5) * 100)}%</span></label>
                            <input type="range" min="0" max="1" step="0.01" value={segment?.zoomKeyframes[editingKeyframeId!]?.positionX ?? 0.5} onChange={(e) => { throttledUpdateZoom({ positionX: Number(e.target.value) }); }} className="w-full accent-[#0079d3]" />
                          </div>
                          <div>
                            <label className="text-sm font-medium text-[#d7dadc] mb-2 flex justify-between"><span>Vertical Position</span><span className="text-[#818384]">{Math.round((segment?.zoomKeyframes[editingKeyframeId!]?.positionY ?? 0.5) * 100)}%</span></label>
                            <input type="range" min="0" max="1" step="0.01" value={segment?.zoomKeyframes[editingKeyframeId!]?.positionY ?? 0.5} onChange={(e) => { throttledUpdateZoom({ positionY: Number(e.target.value) }); }} className="w-full accent-[#0079d3]" />
                          </div>
                        </div>
                      </div>
                    </div>
                  ) : (
                    <div className="bg-[#1a1a1b] rounded-lg border border-[#343536] p-6 flex flex-col items-center justify-center text-center">
                      <div className="bg-[#272729] rounded-full p-3 mb-3"><Search className="w-6 h-6 text-[#818384]" /></div>
                      <p className="text-[#d7dadc] font-medium">This Area Doesn't Have Manual Zoom</p>
                      <p className="text-[#818384] text-sm mt-1 max-w-[200px]">Use your scroll wheel or drag inside the video player to add one</p>
                    </div>
                  )}
                </>
              ) : activePanel === 'background' ? (
                <div className="bg-[#1a1a1b] rounded-lg border border-[#343536] p-4">
                  <h2 className="text-base font-semibold text-[#d7dadc] mb-4">Background & Layout</h2>
                  <div className="space-y-4">
                    <div>
                      <label className="text-sm font-medium text-[#d7dadc] mb-2 flex justify-between">
                        <span>Video Size</span>
                        <span className="text-[#818384]">{backgroundConfig.scale}%</span>
                      </label>
                      <input type="range" min="50" max="100" value={backgroundConfig.scale}
                        onChange={(e) => setBackgroundConfig(prev => ({ ...prev, scale: Number(e.target.value) }))}
                        className="w-full accent-[#0079d3]"
                      />
                    </div>
                    <div>
                      <label className="text-sm font-medium text-[#d7dadc] mb-2 flex justify-between">
                        <span>Roundness</span>
                        <span className="text-[#818384]">{backgroundConfig.borderRadius}px</span>
                      </label>
                      <input type="range" min="0" max="64" value={backgroundConfig.borderRadius}
                        onChange={(e) => setBackgroundConfig(prev => ({ ...prev, borderRadius: Number(e.target.value) }))}
                        className="w-full accent-[#0079d3]"
                      />
                    </div>
                    <div>
                      <label className="text-sm font-medium text-[#d7dadc] mb-2 flex justify-between">
                        <span>Shadow</span>
                        <span className="text-[#818384]">{backgroundConfig.shadow || 0}px</span>
                      </label>
                      <input type="range" min="0" max="100" value={backgroundConfig.shadow || 0}
                        onChange={(e) => setBackgroundConfig(prev => ({ ...prev, shadow: Number(e.target.value) }))}
                        className="w-full accent-[#0079d3]"
                      />
                    </div>
                    <div>
                      <label className="text-sm font-medium text-[#d7dadc] mb-2 flex justify-between">
                        <span>Volume</span>
                        <span className="text-[#818384]">{Math.round((backgroundConfig.volume ?? 1) * 100)}%</span>
                      </label>
                      <input type="range" min="0" max="1" step="0.01" value={backgroundConfig.volume ?? 1}
                        onChange={(e) => setBackgroundConfig(prev => ({ ...prev, volume: Number(e.target.value) }))}
                        className="w-full accent-[#0079d3]"
                      />
                    </div>
                    <div>
                      <label className="text-sm font-medium text-[#d7dadc] mb-3 block">Background Style</label>
                      <div className="grid grid-cols-4 gap-4">
                        {Object.entries(GRADIENT_PRESETS).map(([key, gradient]) => (
                          <button
                            key={key}
                            onClick={() => setBackgroundConfig(prev => ({ ...prev, backgroundType: key as BackgroundConfig['backgroundType'] }))}
                            className={`aspect-square  h-10 rounded-lg transition-all ${gradient} ${backgroundConfig.backgroundType === key
                              ? 'ring-2 ring-[#0079d3] ring-offset-2 ring-offset-[#1a1a1b] scale-105'
                              : 'ring-1 ring-[#343536] hover:ring-[#0079d3]/50'
                              }`}
                          />
                        ))}

                        {/* Upload button - always first */}
                        <label
                          className={`aspect-square h-10 rounded-lg transition-all cursor-pointer
                            ring-1 ring-[#343536] hover:ring-[#0079d3]/50
                            relative overflow-hidden group bg-[#272729]
                          `}
                        >
                          <input
                            type="file"
                            accept="image/*"
                            onChange={handleBackgroundUpload}
                            className="hidden"
                          />
                          <div className="absolute inset-0 flex items-center justify-center">
                            <Upload className="w-5 h-5 text-[#818384] group-hover:text-[#0079d3] transition-colors" />
                          </div>
                        </label>

                        {/* Recent uploads */}
                        {recentUploads.map((imageUrl, index) => (
                          <button
                            key={index}
                            onClick={() => setBackgroundConfig(prev => ({
                              ...prev,
                              backgroundType: 'custom',
                              customBackground: imageUrl
                            }))}
                            className={`aspect-square h-10 rounded-lg transition-all relative overflow-hidden
                              ${backgroundConfig.backgroundType === 'custom' && backgroundConfig.customBackground === imageUrl
                                ? 'ring-2 ring-[#0079d3] ring-offset-2 ring-offset-[#1a1a1b] scale-105'
                                : 'ring-1 ring-[#343536] hover:ring-[#0079d3]/50'
                              }
                            `}
                          >
                            <img
                              src={imageUrl}
                              alt={`Upload ${index + 1}`}
                              className="absolute inset-0 w-full h-full object-cover"
                            />
                          </button>
                        ))}
                      </div>
                    </div>
                  </div>
                </div>
              ) : activePanel === 'cursor' ? (
                <div className="bg-[#1a1a1b] rounded-lg border border-[#343536] p-4">
                  <h2 className="text-base font-semibold text-[#d7dadc] mb-4">Cursor Settings</h2>
                  <div className="space-y-4">
                    <div>
                      <label className="text-sm font-medium text-[#d7dadc] mb-2 flex justify-between">
                        <span>Cursor Size</span>
                        <span className="text-[#818384]">{backgroundConfig.cursorScale ?? 2}x</span>
                      </label>
                      <input
                        type="range"
                        min="1"
                        max="8"
                        step="0.1"
                        value={backgroundConfig.cursorScale ?? 2}
                        onChange={(e) => setBackgroundConfig(prev => ({ ...prev, cursorScale: Number(e.target.value) }))}
                        className="w-full accent-[#0079d3]"
                      />
                    </div>
                    <div>
                      <label className="text-sm font-medium text-[#d7dadc] mb-2 flex justify-between">
                        <span>Movement Smoothing</span>
                        <span className="text-[#818384]">{backgroundConfig.cursorSmoothness ?? 5}</span>
                      </label>
                      <input
                        type="range"
                        min="0"
                        max="10"
                        step="1"
                        value={backgroundConfig.cursorSmoothness ?? 5}
                        onChange={(e) => setBackgroundConfig(prev => ({ ...prev, cursorSmoothness: Number(e.target.value) }))}
                        className="w-full accent-[#0079d3]"
                      />
                    </div>
                  </div>
                </div>
              ) : activePanel === 'text' && (
                <div className="bg-[#1a1a1b] rounded-lg border border-[#343536] p-4">
                  <div className="flex justify-between items-center mb-4">
                    <h2 className="text-base font-semibold text-[#d7dadc]">Text Overlay</h2>
                    <Button
                      onClick={handleAddText}
                      className="bg-[#0079d3] hover:bg-[#0079d3]/90 text-white"
                    >
                      <Type className="w-4 h-4 mr-2" />Add Text
                    </Button>
                  </div>

                  {editingTextId && segment?.textSegments?.find(t => t.id === editingTextId) ? (
                    <div className="space-y-4">
                      <div>
                        <label className="text-sm font-medium text-[#d7dadc] mb-2 block">Text Content</label>
                        <textarea
                          value={segment.textSegments.find(t => t.id === editingTextId)?.text}
                          onChange={(e) => {
                            if (!segment) return;
                            setSegment({
                              ...segment,
                              textSegments: segment.textSegments.map(t =>
                                t.id === editingTextId ? { ...t, text: e.target.value } : t
                              )
                            });
                          }}
                          className="w-full bg-[#272729] border border-[#343536] rounded-md px-3 py-2 text-[#d7dadc]"
                          rows={3}
                        />
                      </div>

                      {/* Shorter helper text */}
                      <div className="bg-[#272729] rounded-lg p-3 text-sm text-[#818384]">
                        <p className="flex items-center gap-2">
                          <span className="bg-[#343536] rounded-full p-1">
                            <Type className="w-4 h-4" />
                          </span>
                          Drag text to reposition
                        </p>
                      </div>

                      <div className="grid grid-cols-2 gap-4">
                        <div>
                          <label className="text-sm font-medium text-[#d7dadc] mb-2 block">Font Size</label>
                          <select
                            value={segment.textSegments.find(t => t.id === editingTextId)?.style.fontSize}
                            onChange={(e) => {
                              if (!segment) return;
                              setSegment({
                                ...segment,
                                textSegments: segment.textSegments.map(t =>
                                  t.id === editingTextId ? { ...t, style: { ...t.style, fontSize: Number(e.target.value) } } : t
                                )
                              });
                            }}
                            className="w-full bg-[#272729] border border-[#343536] rounded-md px-3 py-2 text-[#d7dadc]"
                          >
                            <option value="16">16</option>
                            <option value="24">24</option>
                            <option value="32">32</option>
                            <option value="48">48</option>
                            <option value="64">64</option>
                            <option value="80">80</option>
                            <option value="96">96</option>
                            <option value="128">128</option>
                            <option value="160">160</option>
                            <option value="200">200</option>
                          </select>
                        </div>

                        <div>
                          <label className="text-sm font-medium text-[#d7dadc] mb-2 block">Color</label>
                          <input
                            type="color"
                            value={segment.textSegments.find(t => t.id === editingTextId)?.style.color}
                            onChange={(e) => {
                              if (!segment) return;
                              setSegment({
                                ...segment,
                                textSegments: segment.textSegments.map(t =>
                                  t.id === editingTextId ? { ...t, style: { ...t.style, color: e.target.value } } : t
                                )
                              });
                            }}
                            className="w-12 h-10 bg-[#272729] border border-[#343536] rounded-md p-1"
                          />
                        </div>
                      </div>
                    </div>
                  ) : (
                    <div className="bg-[#1a1a1b] rounded-lg border border-[#343536] p-6 flex flex-col items-center justify-center text-center">
                      <div className="bg-[#272729] rounded-full p-3 mb-3">
                        <Type className="w-6 h-6 text-[#818384]" />
                      </div>
                      <p className="text-[#d7dadc] font-medium">No Text Selected</p>
                      <p className="text-[#818384] text-sm mt-1 max-w-[200px]">
                        Add a new text overlay or select an existing one from the timeline
                      </p>
                    </div>
                  )}
                </div>
              )}
            </div>
          </div>

          <div className="bg-[#1a1a1b] rounded-lg border border-[#343536] p-6">
            <div className="space-y-2 mb-8">
              <div className="flex justify-between items-center">
                <h2 className="text-lg font-semibold text-[#d7dadc]">Timeline</h2>
                <div className="flex gap-2">
                  <Button
                    onClick={() => {
                      if (!segment || !mousePositions.length) return;

                      // Pass actual video dimensions
                      const vid = videoRef.current;
                      if (!vid) return;
                      const motionPath = autoZoomGenerator.generateMotionPath(segment, mousePositions, vid.videoWidth, vid.videoHeight);

                      const newSegment: VideoSegment = {
                        ...segment,
                        zoomKeyframes: segment.zoomKeyframes,
                        smoothMotionPath: motionPath,
                        zoomInfluencePoints: [
                          { time: 0, value: 1.0 },
                          { time: duration, value: 1.0 }
                        ]
                      };

                      setSegment(newSegment);
                      if (currentProjectId) {
                        projectManager.updateProject(currentProjectId, {
                          segment: newSegment,
                          backgroundConfig: backgroundConfig,
                          mousePositions: mousePositions
                        }).then(() => loadProjects());
                      }

                      // Switch to zoom panel
                      setActivePanel('zoom');
                    }}
                    disabled={isProcessing || !currentVideo || !mousePositions.length}
                    className={`flex items-center px-4 py-2 h-9 text-sm font-medium transition-colors ${!currentVideo || isProcessing || !mousePositions.length
                      ? 'bg-gray-600/50 text-gray-400 cursor-not-allowed'
                      : 'bg-green-600 hover:bg-green-700 text-white shadow-sm'
                      }`}
                  >
                    <Wand2 className="w-4 h-4 mr-2" />Auto-Smart Zoom
                  </Button>



                </div>
              </div>
            </div>

            <Timeline
              duration={duration}
              currentTime={currentTime}
              segment={segment}
              thumbnails={thumbnails}
              timelineRef={timelineRef}
              videoRef={videoRef}
              editingKeyframeId={editingKeyframeId}
              editingTextId={editingTextId}
              setCurrentTime={setCurrentTime}
              setEditingKeyframeId={setEditingKeyframeId}
              setEditingTextId={setEditingTextId}
              setActivePanel={setActivePanel}
              setSegment={setSegment}
              onSeek={(time) => videoControllerRef.current?.seek(time)}
            />
          </div>
        </div>
      </main >

      {isProcessing && (
        <div className="fixed inset-0 bg-black/80 flex items-center justify-center z-50">
          <div className="bg-[#1a1a1b] p-6 rounded-lg border border-[#343536]">
            <p className="text-lg text-[#d7dadc]">{exportProgress > 0 ? `Exporting video... ${Math.round(exportProgress)}%` : 'Processing video...'}</p>
          </div>
        </div>
      )
      }

      {/* showConfirmNewRecording removed */}

      {
        showMonitorSelect && (
          <div className="fixed inset-0 bg-black/80 flex items-center justify-center z-50">
            <div className="bg-[#1a1a1b] p-6 rounded-lg border border-[#343536] max-w-md w-full mx-4">
              <h3 className="text-lg font-semibold text-[#d7dadc] mb-4">Select Monitor</h3>
              <div className="space-y-3 mb-6">
                {monitors.map((monitor) => (
                  <button
                    key={monitor.id}
                    onClick={() => {
                      setShowMonitorSelect(false);
                      startNewRecording(monitor.id);
                    }}
                    className="w-full p-4 rounded-lg border border-[#343536] hover:bg-[#272729] transition-colors text-left"
                  >
                    <div className="font-medium text-[#d7dadc]">
                      {monitor.name}
                    </div>
                    <div className="text-sm text-[#818384] mt-1">
                      {monitor.width}x{monitor.height} at ({monitor.x}, {monitor.y})
                    </div>
                  </button>
                ))}
              </div>
              <div className="flex justify-end">
                <Button
                  onClick={() => setShowMonitorSelect(false)}
                  variant="outline"
                  className="bg-transparent border-[#343536] text-[#d7dadc] hover:bg-[#272729] hover:text-[#d7dadc]"
                >
                  Cancel
                </Button>
              </div>
            </div>
          </div>
        )
      }

      {
        currentVideo && !isVideoReady && (
          <div className="absolute inset-0 flex items-center justify-center bg-black/50">
            <div className="text-white">Preparing video...</div>
          </div>
        )
      }

      {
        showExportDialog && (
          <div className="fixed inset-0 bg-black/80 flex items-center justify-center z-50">
            <div className="bg-[#1a1a1b] p-6 rounded-lg border border-[#343536] max-w-md w-full mx-4">
              <h3 className="text-lg font-semibold text-[#d7dadc] mb-4">Export Options</h3>

              <div className="space-y-4 mb-6">
                <div>
                  <label className="text-sm font-medium text-[#d7dadc] mb-2 block">Quality</label>
                  <select
                    value={exportOptions.quality}
                    onChange={(e) => setExportOptions(prev => ({ ...prev, quality: e.target.value as ExportOptions['quality'] }))}
                    className="w-full bg-[#272729] border border-[#343536] rounded-md px-3 py-2 text-[#d7dadc]"
                  >
                    {Object.entries(EXPORT_PRESETS).map(([key, preset]) => (
                      <option key={key} value={key}>{preset.label}</option>
                    ))}
                  </select>
                </div>

                <div>
                  <label className="text-sm font-medium text-[#d7dadc] mb-2 block">Dimensions</label>
                  <select
                    value={exportOptions.dimensions}
                    onChange={(e) => setExportOptions(prev => ({ ...prev, dimensions: e.target.value as ExportOptions['dimensions'] }))}
                    className="w-full bg-[#272729] border border-[#343536] rounded-md px-3 py-2 text-[#d7dadc]"
                  >
                    {Object.entries(DIMENSION_PRESETS).map(([key, preset]) => (
                      <option key={key} value={key}>{preset.label}</option>
                    ))}
                  </select>
                </div>

                <div>
                  <label className="text-sm font-medium text-[#d7dadc] mb-2 block">Speed</label>
                  <div className="bg-[#272729] rounded-md p-3">
                    <div className="flex items-center justify-between mb-3">
                      <div className="flex items-center gap-1.5">
                        <span className="text-sm text-[#d7dadc] tabular-nums">
                          {formatTime(segment ? (segment.trimEnd - segment.trimStart) / exportOptions.speed : 0)}
                        </span>
                        {segment && exportOptions.speed !== 1 && (
                          <span className={`text-xs ${exportOptions.speed > 1 ? 'text-red-400/90' : 'text-green-400/90'}`}>
                            {exportOptions.speed > 1 ? '↓' : '↑'}
                            {formatTime(Math.abs(
                              (segment.trimEnd - segment.trimStart) -
                              ((segment.trimEnd - segment.trimStart) / exportOptions.speed)
                            ))}
                          </span>
                        )}
                      </div>
                      <span className="text-sm font-medium text-[#d7dadc] tabular-nums">
                        {Math.round(exportOptions.speed * 100)}%
                      </span>
                    </div>

                    <div className="flex items-center gap-3">
                      <span className="text-xs text-[#818384] min-w-[36px]">Slower</span>
                      <div className="flex-1">
                        <input
                          type="range"
                          min="50"
                          max="200"
                          step="10"
                          value={exportOptions.speed * 100}
                          onChange={(e) => setExportOptions(prev => ({
                            ...prev,
                            speed: Number(e.target.value) / 100
                          }))}
                          className="w-full h-1 accent-[#0079d3] rounded-full"
                          style={{
                            background: `linear-gradient(to right, 
                            #818384 0%, 
                            #0079d3 ${((exportOptions.speed * 100 - 50) / 150) * 100}%`
                          }}
                        />
                      </div>
                      <span className="text-xs text-[#818384] min-w-[36px]">Faster</span>
                    </div>
                  </div>
                </div>
              </div>

              <div className="flex justify-end gap-3">
                <Button
                  variant="outline"
                  onClick={() => setShowExportDialog(false)}
                  className="bg-transparent border-[#343536] text-[#d7dadc] hover:bg-[#272729] hover:text-[#d7dadc]"
                >
                  Cancel
                </Button>
                <Button
                  onClick={startExport}
                  className="bg-[#0079d3] hover:bg-[#0079d3]/90 text-white"
                >
                  Export Video
                </Button>
              </div>
            </div>
          </div>
        )
      }

      {
        showProjectsDialog && (
          <div className="fixed inset-0 bg-black/80 flex items-center justify-center z-50">
            <div className="bg-[#1a1a1b] p-6 rounded-lg border border-[#343536] max-w-2xl w-full mx-4">
              <div className="flex justify-between items-center mb-6">
                <div className="flex items-center gap-4">
                  <h3 className="text-lg font-semibold text-[#d7dadc]">Recent Projects</h3>
                  <div className="flex items-center gap-2 ml-4">
                    <span className="text-xs text-[#818384]">Limit:</span>
                    <input
                      type="range"
                      min="10"
                      max="100"
                      value={projectManager.getLimit()}
                      onChange={(e) => {
                        projectManager.setLimit(parseInt(e.target.value));
                        loadProjects();
                      }}
                      className="w-24 h-1 bg-[#272729] rounded-lg appearance-none cursor-pointer accent-[#0079d3]"
                    />
                    <span className="text-xs text-[#d7dadc]">{projectManager.getLimit()}</span>
                  </div>
                </div>
                <Button
                  variant="ghost"
                  onClick={() => setShowProjectsDialog(false)}
                  className="text-[#818384] hover:text-[#d7dadc]"
                >
                  ✕
                </Button>
              </div>

              {projects.length === 0 ? (
                <div className="text-center py-8 text-[#818384]">
                  No saved projects yet
                </div>
              ) : (
                <div className="space-y-2 max-h-[60vh] overflow-y-auto">
                  {projects.map((project) => (
                    <div
                      key={project.id}
                      className="flex items-center justify-between p-3 rounded-lg border border-[#343536] hover:bg-[#272729] transition-colors gap-4"
                    >
                      <div className="w-24 h-14 bg-black rounded overflow-hidden flex-shrink-0 border border-[#343536]">
                        {project.thumbnail ? (
                          <img src={project.thumbnail} className="w-full h-full object-cover" alt="Preview" />
                        ) : (
                          <div className="w-full h-full flex items-center justify-center text-[#343536]">
                            <Video className="w-6 h-6" />
                          </div>
                        )}
                      </div>
                      <div className="flex-1 min-w-0">
                        {editingProjectNameId === project.id ? (
                          <input
                            autoFocus
                            className="bg-[#1a1a1b] border border-[#0079d3] rounded px-2 py-1 text-[#d7dadc] w-full"
                            value={projectRenameValue}
                            onChange={(e) => setProjectRenameValue(e.target.value)}
                            onBlur={() => handleRenameProject(project.id)}
                            onKeyDown={(e) => e.key === 'Enter' && handleRenameProject(project.id)}
                            onMouseDown={(e) => e.stopPropagation()}
                          />
                        ) : (
                          <h4
                            className="text-[#d7dadc] font-medium truncate cursor-primary hover:text-[#0079d3] cursor-pointer"
                            title="Click to rename"
                            onClick={() => {
                              setEditingProjectNameId(project.id);
                              setProjectRenameValue(project.name);
                            }}
                          >
                            {project.name}
                          </h4>
                        )}
                        <p className="text-sm text-[#818384]">
                          Last modified: {new Date(project.lastModified).toLocaleDateString()}
                        </p>
                      </div>
                      <div className="flex gap-2">
                        <Button
                          onClick={() => handleLoadProject(project.id)}
                          className="bg-[#0079d3] hover:bg-[#0079d3]/90 text-white"
                        >
                          Load Project
                        </Button>
                        <Button
                          variant="ghost"
                          onClick={async () => {
                            await projectManager.deleteProject(project.id);
                            await loadProjects();
                          }}
                          className="text-red-400 hover:text-red-300 hover:bg-red-900/20"
                        >
                          <Trash2 className="w-4 h-4" />
                        </Button>
                      </div>
                    </div>
                  ))}
                </div>
              )}
            </div>
          </div>
        )
      }

      {
        showHotkeyDialog && (
          <div className="fixed inset-0 bg-black/80 flex items-center justify-center z-50">
            <div className="bg-[#1a1a1b] p-6 rounded-lg border border-[#343536] max-w-sm w-full mx-4 text-center">
              <Keyboard className="w-12 h-12 text-[#0079d3] mx-auto mb-4" />
              <h3 className="text-lg font-semibold text-[#d7dadc] mb-2">
                Press Keys...
              </h3>
              <p className="text-[#818384] mb-6">
                Press the combination of keys you want to use.
              </p>

              <div className="flex justify-center gap-3">
                <Button
                  variant="ghost"
                  onClick={() => { setListeningForKey(false); setShowHotkeyDialog(false); }}
                  className="text-[#d7dadc] hover:bg-[#272729]"
                >
                  Cancel
                </Button>
              </div>
            </div>
          </div>
        )
      }
      {
        (isCheckingDeps || ffmpegMissing || ffprobeMissing || ffmpegInstallStatus.type !== 'Idle') && ffmpegInstallStatus.type !== 'Installed' && (
          <div className="fixed inset-0 bg-black/95 flex items-center justify-center z-[100] backdrop-blur-md">
            <div className="bg-[#1a1a1b] p-8 rounded-2xl border border-[#343536] max-w-md w-full mx-4 shadow-2xl relative overflow-hidden">
              <div className="absolute -top-24 -left-24 w-48 h-48 bg-[#0079d3]/10 rounded-full blur-3xl opacity-50" />
              <div className="absolute -bottom-24 -right-24 w-48 h-48 bg-[#9C17FF]/10 rounded-full blur-3xl opacity-50" />

              <div className="relative z-10 text-center">
                <div className="mb-6 inline-flex p-4 rounded-full bg-[#0079d3]/10 text-[#0079d3]">
                  {ffmpegInstallStatus.type === 'Error' ? (
                    <AlertCircle className="w-10 h-10 text-red-500" />
                  ) : ffmpegInstallStatus.type === 'Downloading' || ffmpegInstallStatus.type === 'Extracting' ? (
                    <Loader2 className="w-10 h-10 animate-spin" />
                  ) : (
                    <Video className="w-10 h-10" />
                  )}
                </div>

                <h3 className="text-2xl font-bold text-white mb-2">
                  {ffmpegInstallStatus.type === 'Downloading' ? 'Downloading Dependencies' :
                    ffmpegInstallStatus.type === 'Extracting' ? 'Setting Up...' :
                      ffmpegInstallStatus.type === 'Error' ? 'Installation Failed' :
                        ffmpegInstallStatus.type === 'Cancelled' ? 'Installation Cancelled' :
                          'Preparing Screen Recorder'}
                </h3>

                <p className="text-[#818384] mb-8 text-sm leading-relaxed">
                  {ffmpegInstallStatus.type === 'Downloading' ? 'FFmpeg and ffprobe are required for screen recording. We are downloading them for you.' :
                    ffmpegInstallStatus.type === 'Extracting' ? 'Almost ready! Extracting binaries to your system.' :
                      ffmpegInstallStatus.type === 'Error' ? ffmpegInstallStatus.message :
                        ffmpegInstallStatus.type === 'Cancelled' ? 'The installation was stopped. You won\'t be able to use the recorder without these dependencies.' :
                          'Please wait while we check your system for required dependencies.'}
                </p>

                {(ffmpegInstallStatus.type === 'Downloading' || ffmpegInstallStatus.type === 'Extracting') && (
                  <div className="space-y-4 mb-8">
                    <div className="h-2 w-full bg-[#272729] rounded-full overflow-hidden">
                      <div
                        className="h-full bg-gradient-to-r from-[#0079d3] to-[#9C17FF] transition-all duration-300 ease-out"
                        style={{ width: `${ffmpegInstallStatus.type === 'Downloading' ? ffmpegInstallStatus.progress : ffmpegInstallStatus.type === 'Extracting' ? 95 : 0}%` }}
                      />
                    </div>
                    {ffmpegInstallStatus.type === 'Downloading' && (
                      <div className="flex justify-between text-xs font-medium">
                        <span className="text-[#0079d3]">{Math.round(ffmpegInstallStatus.progress)}% downloaded</span>
                        <span className="text-[#818384]">FFmpeg Essentials</span>
                      </div>
                    )}
                  </div>
                )}

                <div className="flex flex-col gap-3">
                  {ffmpegInstallStatus.type === 'Error' || ffmpegInstallStatus.type === 'Cancelled' ? (
                    <Button
                      onClick={() => window.location.reload()}
                      className="w-full bg-[#0079d3] hover:bg-[#0079d3]/90 text-white font-semibold py-6 rounded-xl"
                    >
                      Try Again
                    </Button>
                  ) : (
                    <Button
                      variant="ghost"
                      onClick={handleCancelInstall}
                      disabled={ffmpegInstallStatus.type === 'Idle' || ffmpegInstallStatus.type === 'Extracting'}
                      className="w-full text-[#818384] hover:text-white hover:bg-white/5 py-6 rounded-xl border border-[#343536]"
                    >
                      Cancel Installation
                    </Button>
                  )}

                  {(ffmpegInstallStatus.type === 'Error' || ffmpegInstallStatus.type === 'Cancelled') && (
                    <Button
                      variant="ghost"
                      onClick={() => (window as any).ipc.postMessage('close_window')}
                      className="w-full text-red-400 hover:text-red-300 hover:bg-red-500/10 py-6 rounded-xl"
                    >
                      Close App
                    </Button>
                  )}
                </div>
              </div>
            </div>
          </div>
        )
      }
    </div >
  );
}

// Helper function to format time
function formatTime(seconds: number): string {
  const minutes = Math.floor(seconds / 60);
  const remainingSeconds = Math.floor(seconds % 60);
  return `${minutes}:${remainingSeconds.toString().padStart(2, '0')}`;
}

export default App;