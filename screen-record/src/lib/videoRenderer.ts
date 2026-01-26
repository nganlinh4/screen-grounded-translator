import { BackgroundConfig, MousePosition, VideoSegment, ZoomKeyframe, TextSegment } from '@/types/video';

export interface RenderContext {
  video: HTMLVideoElement;
  canvas: HTMLCanvasElement;
  tempCanvas: HTMLCanvasElement;
  segment: VideoSegment;
  backgroundConfig: BackgroundConfig;
  mousePositions: MousePosition[];
  currentTime: number;
}

export interface RenderOptions {
  exportMode?: boolean;
  highQuality?: boolean;
}

interface CursorAnimationState {
  startTime: number;
  isAnimating: boolean;
  progress: number;
  isSquishing: boolean;
  lastPosition?: { x: number; y: number };
}

export class VideoRenderer {
  private animationFrame: number | null = null;
  private isDrawing: boolean = false;
  private cursorAnimation: CursorAnimationState = {
    startTime: 0,
    isAnimating: false,
    progress: 0,
    isSquishing: false
  };
  private SQUISH_DURATION = 100; // Faster initial squish for snappier feel
  private RELEASE_DURATION = 300; // Shorter release for quicker bounce back
  private lastDrawTime: number = 0;
  private readonly FRAME_INTERVAL = 1000 / 120; // Increase to 120fps for smoother animation
  private backgroundConfig: BackgroundConfig | null = null;
  private pointerImage: HTMLImageElement;
  private customBackgroundPattern: CanvasPattern | null = null;
  private lastCustomBackground: string | undefined = undefined;

  private readonly DEFAULT_STATE: ZoomKeyframe = {
    time: 0,
    duration: 0,
    zoomFactor: 1,
    positionX: 0.5,
    positionY: 0.5,
    easingType: 'linear' as const
  };

  private lastCalculatedState: ZoomKeyframe | null = null;
  public getLastCalculatedState() { return this.lastCalculatedState; }

  private smoothedPositions: MousePosition[] | null = null;
  private hasLoggedPositions = false;

  private isDraggingText = false;
  private draggedTextId: string | null = null;
  private dragOffset = { x: 0, y: 0 };

  constructor() {
    // Preload the pointer SVG image.
    this.pointerImage = new Image();
    this.pointerImage.src = '/pointer.svg';
    this.pointerImage.onload = () => { };
  }

  public startAnimation(renderContext: RenderContext) {
    console.log('[VideoRenderer] Starting animation');
    this.stopAnimation();
    this.lastDrawTime = 0;
    this.smoothedPositions = null;

    const animate = () => {
      // Stop animation loop if video is paused
      if (renderContext.video.paused) {
        this.animationFrame = null;
        return;
      }

      const now = performance.now();
      const elapsed = now - this.lastDrawTime;

      if (this.lastDrawTime === 0 || elapsed >= this.FRAME_INTERVAL) {
        this.drawFrame(renderContext)
          .catch(err => console.error('[VideoRenderer] Draw error:', err));
        this.lastDrawTime = now;
      }

      this.animationFrame = requestAnimationFrame(animate);
    };

    this.animationFrame = requestAnimationFrame(animate);
  }

  public stopAnimation() {
    if (this.animationFrame !== null) {
      cancelAnimationFrame(this.animationFrame);
      this.animationFrame = null;
      this.lastDrawTime = 0; // Reset timing when stopping
    }
  }

  public drawFrame = async (
    context: RenderContext,
    options: RenderOptions = {}
  ): Promise<void> => {
    if (this.isDrawing) return;

    const { video, canvas, tempCanvas, segment, backgroundConfig, mousePositions } = context;
    if (!video || !canvas || !segment) return;

    // Store original canvas dimensions
    const targetWidth = canvas.width;
    const targetHeight = canvas.height;

    // Temporarily set canvas to video dimensions for consistent rendering
    canvas.width = video.videoWidth;
    canvas.height = video.videoHeight;
    tempCanvas.width = video.videoWidth;
    tempCanvas.height = video.videoHeight;

    const isExportMode = options.exportMode || false;
    const quality = isExportMode ? 'high' : 'medium';

    const ctx = canvas.getContext('2d', {
      alpha: false,
      willReadFrequently: false
    });
    if (!ctx) return;

    ctx.imageSmoothingQuality = quality;
    this.isDrawing = true;

    try {
      // Calculate dimensions once
      const crop = (backgroundConfig.cropBottom || 0) / 100;
      const scale = backgroundConfig.scale / 100;
      const scaledWidth = canvas.width * scale;
      const scaledHeight = (canvas.height * (1 - crop)) * scale;
      const x = (canvas.width - scaledWidth) / 2;
      const y = (canvas.height - scaledHeight) / 2;
      const zoomState = this.calculateCurrentZoomState(video.currentTime, segment);

      ctx.save();

      // Apply zoom transformation to entire canvas before drawing anything
      if (zoomState && zoomState.zoomFactor !== 1) {
        const zoomedWidth = canvas.width * zoomState.zoomFactor;
        const zoomedHeight = canvas.height * zoomState.zoomFactor;
        const zoomOffsetX = (canvas.width - zoomedWidth) * zoomState.positionX;
        const zoomOffsetY = (canvas.height - zoomedHeight) * zoomState.positionY;

        ctx.translate(zoomOffsetX, zoomOffsetY);
        ctx.scale(zoomState.zoomFactor, zoomState.zoomFactor);
      }

      // Draw background first
      ctx.fillStyle = this.getBackgroundStyle(
        ctx,
        backgroundConfig.backgroundType,
        backgroundConfig.customBackground
      );
      ctx.fillRect(0, 0, canvas.width, canvas.height);

      // Setup temporary canvas for rounded corners and shadows
      tempCanvas.width = canvas.width;
      tempCanvas.height = canvas.height;
      const tempCtx = tempCanvas.getContext('2d', {
        alpha: true,
        willReadFrequently: false
      });
      if (!tempCtx) return;

      // Clear temp canvas
      tempCtx.clearRect(0, 0, canvas.width, canvas.height);

      // Draw video frame with rounded corners to temp canvas
      tempCtx.save();

      // Improve anti-aliasing
      tempCtx.imageSmoothingEnabled = true;
      tempCtx.imageSmoothingQuality = 'high';

      // Create path for the rounded rectangle
      const radius = backgroundConfig.borderRadius;
      const offset = 0.5;

      // Draw shadow first if enabled
      if (backgroundConfig.shadow) {
        tempCtx.save();

        // Set shadow properties
        tempCtx.shadowColor = 'rgba(0, 0, 0, 0.5)';
        tempCtx.shadowBlur = backgroundConfig.shadow;
        tempCtx.shadowOffsetY = backgroundConfig.shadow * 0.5;

        // Create the rounded rectangle path
        tempCtx.beginPath();
        tempCtx.moveTo(x + radius + offset, y + offset);
        tempCtx.lineTo(x + scaledWidth - radius - offset, y + offset);
        tempCtx.quadraticCurveTo(x + scaledWidth - offset, y + offset, x + scaledWidth - offset, y + radius + offset);
        tempCtx.lineTo(x + scaledWidth - offset, y + scaledHeight - radius - offset);
        tempCtx.quadraticCurveTo(x + scaledWidth - offset, y + scaledHeight - offset, x + scaledWidth - radius - offset, y + scaledHeight - offset);
        tempCtx.lineTo(x + radius + offset, y + scaledHeight - offset);
        tempCtx.quadraticCurveTo(x + offset, y + scaledHeight - offset, x + offset, y + scaledHeight - radius - offset);
        tempCtx.lineTo(x + offset, y + radius + offset);
        tempCtx.quadraticCurveTo(x + offset, y + offset, x + radius + offset, y + offset);
        tempCtx.closePath();

        // Fill with white to create shadow
        tempCtx.fillStyle = '#fff';
        tempCtx.fill();

        tempCtx.restore();
      }

      // Now draw the actual video content
      tempCtx.beginPath();
      tempCtx.moveTo(x + radius + offset, y + offset);
      tempCtx.lineTo(x + scaledWidth - radius - offset, y + offset);
      tempCtx.quadraticCurveTo(x + scaledWidth - offset, y + offset, x + scaledWidth - offset, y + radius + offset);
      tempCtx.lineTo(x + scaledWidth - offset, y + scaledHeight - radius - offset);
      tempCtx.quadraticCurveTo(x + scaledWidth - offset, y + scaledHeight - offset, x + scaledWidth - radius - offset, y + scaledHeight - offset);
      tempCtx.lineTo(x + radius + offset, y + scaledHeight - offset);
      tempCtx.quadraticCurveTo(x + offset, y + scaledHeight - offset, x + offset, y + scaledHeight - radius - offset);
      tempCtx.lineTo(x + offset, y + radius + offset);
      tempCtx.quadraticCurveTo(x + offset, y + offset, x + radius + offset, y + offset);
      tempCtx.closePath();

      // Clip and draw the video (using 9-arg version to crop bottom)
      tempCtx.clip();
      tempCtx.drawImage(
        video,
        0, 0, video.videoWidth, video.videoHeight * (1 - crop), // sx, sy, sWidth, sHeight
        x, y, scaledWidth, scaledHeight                      // dx, dy, dWidth, dHeight
      );

      // Add a subtle border to smooth out edges
      tempCtx.strokeStyle = 'rgba(0, 0, 0, 0.1)';
      tempCtx.lineWidth = 1;
      tempCtx.stroke();

      tempCtx.restore();

      // Composite temp canvas onto main canvas
      ctx.drawImage(tempCanvas, 0, 0);

      // Mouse cursor
      const interpolatedPosition = this.interpolateCursorPosition(
        video.currentTime,
        mousePositions
      );
      if (interpolatedPosition) {
        // Save current transform
        ctx.save();
        // Reset the transform before drawing cursor
        ctx.setTransform(1, 0, 0, 1, 0, 0);

        // Map mouse position to the cropped video space
        // original normalized mouse pos (0-1) on the UNcropped video
        // We need to check if mouse is within the uncropped Y range
        const mouseNormY = interpolatedPosition.y / video.videoHeight;

        // If mouse is below crop line, it disappears
        if (mouseNormY <= (1 - crop)) {
          // Re-calculate cursor position in terms of the cropped video frame drawn at (x, y)
          let cursorX = x + (interpolatedPosition.x * scaledWidth / video.videoWidth);
          // For Y, we scale it relative to the CROPPED height
          let cursorY = y + (interpolatedPosition.y * scaledHeight / (video.videoHeight * (1 - crop)));

          // If there's zoom, adjust cursor position
          if (zoomState && zoomState.zoomFactor !== 1) {
            // Apply the same zoom transformation to cursor position
            cursorX = cursorX * zoomState.zoomFactor + (canvas.width - canvas.width * zoomState.zoomFactor) * zoomState.positionX;
            cursorY = cursorY * zoomState.zoomFactor + (canvas.height - canvas.height * zoomState.zoomFactor) * zoomState.positionY;
          }

          // Scale cursor size based on video dimensions ratio and zoom
          const sizeRatio = Math.min(targetWidth / video.videoWidth, targetHeight / video.videoHeight);
          const cursorScale = (backgroundConfig.cursorScale || 2) * sizeRatio * (zoomState?.zoomFactor || 1);

          this.drawMouseCursor(
            ctx,
            cursorX,
            cursorY,
            interpolatedPosition.isClicked || false,
            cursorScale,
            interpolatedPosition.cursor_type || 'default'
          );
        }

        // Restore transform
        ctx.restore();
      }
      // Performance tracking removed

      this.backgroundConfig = context.backgroundConfig;

      // Add text overlays
      if (segment.textSegments) {
        for (const textSegment of segment.textSegments) {
          if (video.currentTime >= textSegment.startTime && video.currentTime <= textSegment.endTime) {
            this.drawTextOverlay(ctx, textSegment, canvas.width, canvas.height);
          }
        }
      }

    } finally {
      this.isDrawing = false;
      ctx.restore();

      // If we're exporting and dimensions are different
      if (options.exportMode && (targetWidth !== video.videoWidth || targetHeight !== video.videoHeight)) {
        // Create a temporary canvas for scaling
        const exportCanvas = document.createElement('canvas');
        exportCanvas.width = targetWidth;
        exportCanvas.height = targetHeight;
        const exportCtx = exportCanvas.getContext('2d', {
          alpha: false,
          willReadFrequently: false
        });

        if (exportCtx) {
          // Use better quality settings for export
          exportCtx.imageSmoothingEnabled = true;
          exportCtx.imageSmoothingQuality = 'high';

          exportCtx.drawImage(canvas, 0, 0, targetWidth, targetHeight);
          // Copy scaled content back to main canvas
          canvas.width = targetWidth;
          canvas.height = targetHeight;
          ctx?.drawImage(exportCanvas, 0, 0);
          exportCanvas.remove(); // Clean up
        }
      } else if (!options.exportMode) {
        // For preview, restore original canvas size with proper scaling
        const previewCanvas = document.createElement('canvas');
        previewCanvas.width = targetWidth;
        previewCanvas.height = targetHeight;
        const previewCtx = previewCanvas.getContext('2d', {
          alpha: false,
          willReadFrequently: false
        });

        if (previewCtx) {
          previewCtx.imageSmoothingEnabled = true;
          previewCtx.imageSmoothingQuality = 'high';
          previewCtx.drawImage(canvas, 0, 0, targetWidth, targetHeight);

          canvas.width = targetWidth;
          canvas.height = targetHeight;
          ctx?.drawImage(previewCanvas, 0, 0);
          previewCanvas.remove(); // Clean up
        }
      }
    }
  };

  private getBackgroundStyle(
    ctx: CanvasRenderingContext2D,
    type: BackgroundConfig['backgroundType'],
    customBackground?: string
  ): string | CanvasGradient | CanvasPattern {
    switch (type) {
      case 'gradient1': {
        // Blue to violet gradient
        const gradient = ctx.createLinearGradient(0, 0, ctx.canvas.width, 0); // horizontal gradient
        gradient.addColorStop(0, '#2563eb'); // blue-600
        gradient.addColorStop(1, '#7c3aed'); // violet-600
        return gradient;
      }
      case 'gradient2': {
        // Rose to orange gradient
        const gradient = ctx.createLinearGradient(0, 0, ctx.canvas.width, 0);
        gradient.addColorStop(0, '#fb7185'); // rose-400
        gradient.addColorStop(1, '#fdba74'); // orange-300
        return gradient;
      }
      case 'gradient3': {
        // Emerald to teal gradient
        const gradient = ctx.createLinearGradient(0, 0, ctx.canvas.width, 0);
        gradient.addColorStop(0, '#10b981'); // emerald-500
        gradient.addColorStop(1, '#2dd4bf'); // teal-400
        return gradient;
      }
      case 'custom': {
        if (customBackground) {
          // Only create new pattern if background changed
          if (this.lastCustomBackground !== customBackground || !this.customBackgroundPattern) {
            const img = new Image();
            img.src = customBackground;

            if (img.complete) {
              // Create a temporary canvas for scaling the background
              const tempCanvas = document.createElement('canvas');
              const tempCtx = tempCanvas.getContext('2d');

              if (tempCtx) {
                // Scale the image to a reasonable size (e.g., viewport width)
                const targetWidth = Math.min(1920, window.innerWidth);
                const scale = targetWidth / img.width;
                const targetHeight = img.height * scale;

                tempCanvas.width = targetWidth;
                tempCanvas.height = targetHeight;

                // Use better quality settings
                tempCtx.imageSmoothingEnabled = true;
                tempCtx.imageSmoothingQuality = 'high';

                // Draw scaled image
                tempCtx.drawImage(img, 0, 0, targetWidth, targetHeight);

                // Create pattern from scaled image
                this.customBackgroundPattern = ctx.createPattern(tempCanvas, 'repeat');
                this.lastCustomBackground = customBackground;

                // Clean up
                tempCanvas.remove();
              }
            }
          }

          if (this.customBackgroundPattern) {
            // Reset pattern transform
            this.customBackgroundPattern.setTransform(new DOMMatrix());

            // Calculate scale to maintain aspect ratio
            const scale = Math.max(
              ctx.canvas.width / window.innerWidth,
              ctx.canvas.height / window.innerHeight
            ) * 1.1; // Slightly larger to avoid gaps

            // Apply transform to pattern
            const matrix = new DOMMatrix()
              .scale(scale);
            this.customBackgroundPattern.setTransform(matrix);

            return this.customBackgroundPattern;
          }
        }
        return '#000000'; // Fallback
      }
      case 'solid': {
        // Create a subtle dark gradient
        const gradient = ctx.createLinearGradient(0, 0, 0, ctx.canvas.height);
        gradient.addColorStop(0, '#0a0a0a'); // Very slightly lighter black at top
        gradient.addColorStop(0.5, '#000000'); // Pure black in middle
        gradient.addColorStop(1, '#0a0a0a'); // Very slightly lighter black at bottom

        // Add a subtle radial overlay for more depth
        const centerX = ctx.canvas.width / 2;
        const centerY = ctx.canvas.height / 2;
        const radialGradient = ctx.createRadialGradient(
          centerX, centerY, 0,
          centerX, centerY, ctx.canvas.width * 0.8
        );
        radialGradient.addColorStop(0, 'rgba(30, 30, 30, 0.15)'); // Subtle light center
        radialGradient.addColorStop(1, 'rgba(0, 0, 0, 0)'); // Fade to transparent

        // Draw base gradient
        ctx.fillStyle = gradient;
        ctx.fillRect(0, 0, ctx.canvas.width, ctx.canvas.height);

        // Add radial overlay
        ctx.fillStyle = radialGradient;
        ctx.fillRect(0, 0, ctx.canvas.width, ctx.canvas.height);

        return 'rgba(0,0,0,0)'; // Return transparent as we've already filled
      }
      default:
        return '#000000';
    }
  }

  private calculateCurrentZoomState(
    currentTime: number,
    segment: VideoSegment
  ): ZoomKeyframe {
    const state = this.calculateCurrentZoomStateInternal(currentTime, segment);
    this.lastCalculatedState = state;
    return state;
  }

  private calculateCurrentZoomStateInternal(
    currentTime: number,
    segment: VideoSegment
  ): ZoomKeyframe {
    // Priority: Continuous Motion Path (Smart Zoom)
    if (segment.smoothMotionPath && segment.smoothMotionPath.length > 0) {
      const path = segment.smoothMotionPath;

      // Find bounding frames
      // Binary search would be faster but linear is ok for this data size
      const idx = path.findIndex(p => p.time >= currentTime);

      let cam = { x: 1920 / 2, y: 1080 / 2, zoom: 1.0 };

      if (idx === -1) {
        // Past end
        const last = path[path.length - 1];
        cam = { x: last.x, y: last.y, zoom: last.zoom };
      } else if (idx === 0) {
        // Before start
        const first = path[0];
        cam = { x: first.x, y: first.y, zoom: first.zoom };
      } else {
        // Interpolate
        const p1 = path[idx - 1];
        const p2 = path[idx];
        const t = (currentTime - p1.time) / (p2.time - p1.time);

        cam = {
          x: p1.x + (p2.x - p1.x) * t,
          y: p1.y + (p2.y - p1.y) * t,
          zoom: p1.zoom + (p2.zoom - p1.zoom) * t
        };
      }

      // Apply Zoom Influence Curve (if exists)
      if (segment.zoomInfluencePoints && segment.zoomInfluencePoints.length > 0) {
        const points = segment.zoomInfluencePoints;
        let influence = 1.0;

        // Find bounding points
        // Assuming sorted by time
        const iIdx = points.findIndex(p => p.time >= currentTime);

        if (iIdx === -1) {
          influence = points[points.length - 1].value;
        } else if (iIdx === 0) {
          influence = points[0].value;
        } else {
          const ip1 = points[iIdx - 1];
          const ip2 = points[iIdx];
          const it = (currentTime - ip1.time) / (ip2.time - ip1.time);

          // Cosine Interpolation for smoothness
          const cosT = (1 - Math.cos(it * Math.PI)) / 2;
          influence = ip1.value * (1 - cosT) + ip2.value * cosT;
        }

        // Blend Physics Camera with Overview (Zoom 1, Center) based on influence
        // influence 1.0 = Full Physics
        // influence 0.0 = Overview

        cam.zoom = 1.0 + (cam.zoom - 1.0) * influence;
        cam.x = (1920 / 2) + (cam.x - (1920 / 2)) * influence;
        cam.y = (1080 / 2) + (cam.y - (1080 / 2)) * influence;
      }

      let resultState: ZoomKeyframe = {
        time: currentTime,
        duration: 0,
        zoomFactor: cam.zoom,
        positionX: cam.x / 1920,
        positionY: cam.y / 1080,
        easingType: 'linear'
      };

      // MANUAL KEYFRAME BLENDING (Real-time Feedback)
      if (segment.zoomKeyframes && segment.zoomKeyframes.length > 0) {
        const WINDOW = 1.5;
        const nearby = segment.zoomKeyframes
          .map(kf => ({ kf, dist: Math.abs(kf.time - currentTime) }))
          .filter(item => item.dist < WINDOW)
          .sort((a, b) => a.dist - b.dist)[0];

        if (nearby) {
          const ratio = nearby.dist / WINDOW;
          const weight = (1 + Math.cos(ratio * Math.PI)) / 2;

          resultState.zoomFactor = resultState.zoomFactor * (1 - weight) + nearby.kf.zoomFactor * weight;
          resultState.positionX = resultState.positionX * (1 - weight) + nearby.kf.positionX * weight;
          resultState.positionY = resultState.positionY * (1 - weight) + nearby.kf.positionY * weight;
        }
      }

      return resultState;
    }

    // Fallback: Standard Keyframes
    const sortedKeyframes = [...segment.zoomKeyframes].sort((a, b) => a.time - b.time);
    if (sortedKeyframes.length === 0) return this.DEFAULT_STATE;

    const nextKeyframe = sortedKeyframes.find(k => k.time > currentTime);
    const prevKeyframe = [...sortedKeyframes].reverse().find(k => k.time <= currentTime);

    const TRANSITION_DURATION = 1.0;

    // If we have a previous keyframe and next keyframe that are close
    if (prevKeyframe && nextKeyframe && (nextKeyframe.time - prevKeyframe.time) <= TRANSITION_DURATION) {
      const progress = (currentTime - prevKeyframe.time) / (nextKeyframe.time - prevKeyframe.time);
      const easedProgress = this.easeOutCubic(Math.min(1, Math.max(0, progress)));

      return {
        time: currentTime,
        duration: nextKeyframe.time - prevKeyframe.time,
        zoomFactor: prevKeyframe.zoomFactor + (nextKeyframe.zoomFactor - prevKeyframe.zoomFactor) * easedProgress,
        positionX: prevKeyframe.positionX + (nextKeyframe.positionX - prevKeyframe.positionX) * easedProgress,
        positionY: prevKeyframe.positionY + (nextKeyframe.positionY - prevKeyframe.positionY) * easedProgress,
        easingType: 'easeOut' as const
      };
    }

    // If approaching next keyframe
    if (nextKeyframe) {
      const timeToNext = nextKeyframe.time - currentTime;
      if (timeToNext <= TRANSITION_DURATION) {
        const progress = (TRANSITION_DURATION - timeToNext) / TRANSITION_DURATION;
        const easedProgress = this.easeOutCubic(Math.min(1, Math.max(0, progress)));

        const startState = prevKeyframe || this.DEFAULT_STATE;

        return {
          time: currentTime,
          duration: TRANSITION_DURATION,
          zoomFactor: startState.zoomFactor + (nextKeyframe.zoomFactor - startState.zoomFactor) * easedProgress,
          positionX: startState.positionX + (nextKeyframe.positionX - startState.positionX) * easedProgress,
          positionY: startState.positionY + (nextKeyframe.positionY - startState.positionY) * easedProgress,
          easingType: 'easeOut' as const
        };
      }
    }

    // If we have a previous keyframe, maintain its state
    if (prevKeyframe) {
      return prevKeyframe;
    }

    return this.DEFAULT_STATE;
  }

  private easeOutCubic(x: number): number {
    return 1 - Math.pow(1 - x, 3);
  }

  private catmullRomInterpolate(
    p0: number,
    p1: number,
    p2: number,
    p3: number,
    t: number
  ): number {
    const t2 = t * t;
    const t3 = t2 * t;

    return 0.5 * (
      (2 * p1) +
      (-p0 + p2) * t +
      (2 * p0 - 5 * p1 + 4 * p2 - p3) * t2 +
      (-p0 + 3 * p1 - 3 * p2 + p3) * t3
    );
  }

  private smoothMousePositions(
    positions: MousePosition[],
    targetFps: number = 120
  ): MousePosition[] {
    if (positions.length < 4) return positions;

    const smoothed: MousePosition[] = [];

    // First pass: Catmull-Rom interpolation
    for (let i = 0; i < positions.length - 3; i++) {
      const p0 = positions[i];
      const p1 = positions[i + 1];
      const p2 = positions[i + 2];
      const p3 = positions[i + 3];

      const segmentDuration = p2.timestamp - p1.timestamp;
      const numFrames = Math.ceil(segmentDuration * targetFps);

      for (let frame = 0; frame < numFrames; frame++) {
        const t = frame / numFrames;
        const timestamp = p1.timestamp + (segmentDuration * t);

        const x = this.catmullRomInterpolate(p0.x, p1.x, p2.x, p3.x, t);
        const y = this.catmullRomInterpolate(p0.y, p1.y, p2.y, p3.y, t);
        const isClicked = Boolean(p1.isClicked || p2.isClicked);
        // Use the cursor type from the nearest position
        const cursor_type = t < 0.5 ? p1.cursor_type : p2.cursor_type;

        smoothed.push({ x, y, timestamp, isClicked, cursor_type });
      }
    }

    // Get smoothness value from background config, default to 5 if not set
    // Scale it up to make the effect more noticeable (1-10 becomes 2-20)
    const windowSize = ((this.backgroundConfig?.cursorSmoothness || 5) * 2) + 1;

    // Multiple smoothing passes based on smoothness value
    const passes = Math.ceil(windowSize / 2);
    let currentSmoothed = smoothed;

    // Apply multiple passes of smoothing based on the smoothness value
    for (let pass = 0; pass < passes; pass++) {
      const passSmoothed: MousePosition[] = [];

      for (let i = 0; i < currentSmoothed.length; i++) {
        let sumX = 0;
        let sumY = 0;
        let totalWeight = 0;

        // Keep cursor type from original position
        const cursor_type = currentSmoothed[i].cursor_type;

        // Only smooth position, not cursor type
        for (let j = Math.max(0, i - windowSize); j <= Math.min(currentSmoothed.length - 1, i + windowSize); j++) {
          const distance = Math.abs(i - j);
          const weight = Math.exp(-distance * (0.5 / windowSize));

          sumX += currentSmoothed[j].x * weight;
          sumY += currentSmoothed[j].y * weight;
          totalWeight += weight;
        }

        passSmoothed.push({
          x: sumX / totalWeight,
          y: sumY / totalWeight,
          timestamp: currentSmoothed[i].timestamp,
          isClicked: currentSmoothed[i].isClicked,
          cursor_type // Preserve the cursor type
        });
      }

      currentSmoothed = passSmoothed;
    }

    // Apply threshold to remove tiny movements
    // Make threshold smaller for higher smoothness values
    const threshold = 0.5 / (windowSize / 2); // Adjust threshold based on smoothness
    let lastSignificantPos = currentSmoothed[0];
    const finalSmoothed = [lastSignificantPos];

    for (let i = 1; i < currentSmoothed.length; i++) {
      const current = currentSmoothed[i];
      const distance = Math.sqrt(
        Math.pow(current.x - lastSignificantPos.x, 2) +
        Math.pow(current.y - lastSignificantPos.y, 2)
      );

      if (distance > threshold || current.isClicked !== lastSignificantPos.isClicked) {
        finalSmoothed.push(current);
        lastSignificantPos = current;
      } else {
        finalSmoothed.push({
          ...lastSignificantPos,
          timestamp: current.timestamp
        });
      }
    }

    return finalSmoothed;
  }

  private interpolateCursorPosition(
    currentTime: number,
    mousePositions: MousePosition[],
  ): { x: number; y: number; isClicked: boolean; cursor_type: string } | null {
    if (mousePositions.length === 0) return null;

    // Add cursor type frequency analysis
    if (!this.hasLoggedPositions) {
      this.hasLoggedPositions = true;
    }

    // Cache smoothed positions
    if (!this.smoothedPositions || this.smoothedPositions.length === 0) {
      this.smoothedPositions = this.smoothMousePositions(mousePositions);
    }

    const positions = this.smoothedPositions;

    // Find the exact position for the current time
    const exactMatch = positions.find(pos => Math.abs(pos.timestamp - currentTime) < 0.001);
    if (exactMatch) {

      return {
        x: exactMatch.x,
        y: exactMatch.y,
        isClicked: Boolean(exactMatch.isClicked),
        cursor_type: exactMatch.cursor_type || 'default'
      };
    }

    // Find the two closest positions
    const nextIndex = positions.findIndex(pos => pos.timestamp > currentTime);
    if (nextIndex === -1) {
      const last = positions[positions.length - 1];
      return {
        x: last.x,
        y: last.y,
        isClicked: Boolean(last.isClicked),
        cursor_type: last.cursor_type || 'default'
      };
    }

    if (nextIndex === 0) {
      const first = positions[0];
      return {
        x: first.x,
        y: first.y,
        isClicked: Boolean(first.isClicked),
        cursor_type: first.cursor_type || 'default'
      };
    }

    // Linear interpolation between the two closest points
    const prev = positions[nextIndex - 1];
    const next = positions[nextIndex];
    const t = (currentTime - prev.timestamp) / (next.timestamp - prev.timestamp);


    return {
      x: prev.x + (next.x - prev.x) * t,
      y: prev.y + (next.y - prev.y) * t,
      isClicked: Boolean(prev.isClicked || next.isClicked),
      cursor_type: next.cursor_type || 'default'
    };
  }

  private drawMouseCursor(
    ctx: CanvasRenderingContext2D,
    x: number,
    y: number,
    isClicked: boolean,
    scale: number = 2,
    cursorType: string = 'default'
  ) {
    ctx.save();
    this.drawCursorShape(ctx, x, y, isClicked, scale, cursorType);
    ctx.restore();
  }

  private drawCursorShape(
    ctx: CanvasRenderingContext2D,
    x: number,
    y: number,
    isClicked: boolean,
    scale: number = 2,
    cursorType: string
  ) {
    const lowerType = cursorType.toLowerCase();


    ctx.save();
    ctx.translate(x, y);
    ctx.scale(scale, scale);

    // Handle click animation state
    const now = performance.now();
    if (isClicked && !this.cursorAnimation.isAnimating) {
      // Start new click animation
      this.cursorAnimation.startTime = now;
      this.cursorAnimation.isAnimating = true;
      this.cursorAnimation.isSquishing = true;
    }

    // Apply animation transforms
    if (this.cursorAnimation.isAnimating) {
      const elapsed = now - this.cursorAnimation.startTime;

      if (this.cursorAnimation.isSquishing) {
        // Squish phase
        const progress = Math.min(1, elapsed / this.SQUISH_DURATION);
        const scaleAmount = 1 - (0.2 * this.easeOutQuad(progress)); // Reduce scale by 20%
        ctx.scale(scaleAmount, scaleAmount);

        if (progress >= 1) {
          // Switch to release phase
          this.cursorAnimation.isSquishing = false;
          this.cursorAnimation.startTime = now;
        }
      } else {
        // Release/bounce phase
        const progress = Math.min(1, elapsed / this.RELEASE_DURATION);
        const baseScale = 0.8 + (0.2 * this.easeOutBack(progress));
        ctx.scale(baseScale, baseScale);

        if (progress >= 1) {
          this.cursorAnimation.isAnimating = false;
        }
      }
    }


    switch (lowerType) {
      case 'text': {

        ctx.translate(-6, -8);

        // I-beam cursor with more detailed shape
        const ibeam = new Path2D(`
          M 2 0 L 10 0 L 10 2 L 7 2 L 7 14 L 10 14 L 10 16 L 2 16 L 2 14 L 5 14 L 5 2 L 2 2 Z
        `);

        // White outline
        ctx.strokeStyle = 'white';
        ctx.lineWidth = 1.5;
        ctx.stroke(ibeam);

        // Black fill
        ctx.fillStyle = 'black';
        ctx.fill(ibeam);
        break;
      }

      case 'pointer': {

        // If the pointer image is loaded, draw it. Use fallback dimensions if necessary.
        let imgWidth = 24, imgHeight = 24;
        if (this.pointerImage.complete && this.pointerImage.naturalWidth > 0) {
          imgWidth = this.pointerImage.naturalWidth;
          imgHeight = this.pointerImage.naturalHeight;
        }

        // Shift the image offset to center the pointer tip
        // Adjust offsetX and offsetY as needed. Here we shift right and down by 4 pixels each.
        const offsetX = 8;
        const offsetY = 16;
        ctx.translate(-imgWidth / 2 + offsetX, -imgHeight / 2 + offsetY);
        ctx.drawImage(this.pointerImage, 0, 0, imgWidth, imgHeight);
        break;
      }

      default: {

        ctx.translate(-8, -5);
        const mainArrow = new Path2D('M 8.2 4.9 L 19.8 16.5 L 13 16.5 L 12.6 16.6 L 8.2 20.9 Z');
        const clickIndicator = new Path2D('M 17.3 21.6 L 13.7 23.1 L 9 12 L 12.7 10.5 Z');

        ctx.strokeStyle = 'white';
        ctx.lineWidth = 1.5;
        ctx.stroke(mainArrow);
        ctx.stroke(clickIndicator);

        ctx.fillStyle = 'black';
        ctx.fill(mainArrow);
        ctx.fill(clickIndicator);
        break;
      }
    }

    ctx.restore();
  }

  // Helper methods for animations
  private easeOutQuad(t: number): number {
    return t * (2 - t);
  }

  private easeOutBack(t: number): number {
    const c1 = 1.70158;
    const c3 = c1 + 1;
    return 1 + c3 * Math.pow(t - 1, 3) + c1 * Math.pow(t - 1, 2);
  }

  private drawTextOverlay(
    ctx: CanvasRenderingContext2D,
    textSegment: TextSegment,
    width: number,
    height: number
  ) {
    ctx.save();

    // Configure text style
    ctx.font = `${textSegment.style.fontSize}px sans-serif`;
    ctx.fillStyle = textSegment.style.color;
    ctx.textAlign = 'center';

    const x = (textSegment.style.x / 100) * width;
    const y = (textSegment.style.y / 100) * height;

    // Draw hit area for dragging (invisible)
    const metrics = ctx.measureText(textSegment.text);
    const textHeight = textSegment.style.fontSize;
    const hitArea = {
      x: x - metrics.width / 2 - 10,
      y: y - textHeight - 10,
      width: metrics.width + 20,
      height: textHeight + 20
    };

    // Optional: show hit area when text is being dragged
    if (this.draggedTextId === textSegment.id) {
      ctx.fillStyle = 'rgba(0, 121, 211, 0.1)';
      ctx.fillRect(hitArea.x, hitArea.y, hitArea.width, hitArea.height);
    }

    // Draw text with shadow
    ctx.shadowColor = 'rgba(0,0,0,0.7)';
    ctx.shadowBlur = 6;
    ctx.shadowOffsetX = 2;
    ctx.shadowOffsetY = 2;
    ctx.fillStyle = textSegment.style.color;
    ctx.fillText(textSegment.text, x, y);

    ctx.restore();

    // Return hit area for collision detection
    return hitArea;
  }

  public handleMouseDown(e: MouseEvent, segment: VideoSegment, canvas: HTMLCanvasElement) {
    const rect = canvas.getBoundingClientRect();
    const x = (e.clientX - rect.left) * (canvas.width / rect.width);
    const y = (e.clientY - rect.top) * (canvas.height / rect.height);

    // Check each text segment for collision
    for (const text of segment.textSegments) {
      const ctx = canvas.getContext('2d');
      if (!ctx) return;

      const hitArea = this.drawTextOverlay(ctx, text, canvas.width, canvas.height);
      if (x >= hitArea.x && x <= hitArea.x + hitArea.width &&
        y >= hitArea.y && y <= hitArea.y + hitArea.height) {
        this.isDraggingText = true;
        this.draggedTextId = text.id;
        this.dragOffset.x = x - (text.style.x / 100 * canvas.width);
        this.dragOffset.y = y - (text.style.y / 100 * canvas.height);
        canvas.style.cursor = 'move';
        break;
      }
    }
  }

  public handleMouseMove(
    e: MouseEvent,
    _segment: VideoSegment,
    canvas: HTMLCanvasElement,
    onTextMove: (id: string, x: number, y: number) => void
  ) {
    if (!this.isDraggingText || !this.draggedTextId) return;

    const rect = canvas.getBoundingClientRect();
    const x = (e.clientX - rect.left) * (canvas.width / rect.width);
    const y = (e.clientY - rect.top) * (canvas.height / rect.height);

    const newX = Math.max(0, Math.min(100, ((x - this.dragOffset.x) / canvas.width) * 100));
    const newY = Math.max(0, Math.min(100, ((y - this.dragOffset.y) / canvas.height) * 100));

    onTextMove(this.draggedTextId, newX, newY);
  }

  public handleMouseUp(canvas: HTMLCanvasElement) {
    this.isDraggingText = false;
    this.draggedTextId = null;
    canvas.style.cursor = 'default';
  }
}

// Create and export a singleton instance
export const videoRenderer = new VideoRenderer(); 