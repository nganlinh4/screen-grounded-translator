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

export class VideoRenderer {
  private animationFrame: number | null = null;
  private isDrawing: boolean = false;
  private lastDrawTime: number = 0;
  private latestElapsed: number = 0;
  private readonly FRAME_INTERVAL = 1000 / 120; // 120fps target
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

  // Smooth cursor animation state
  private currentSquishScale = 1.0;
  private lastHoldTime = -1;
  private readonly CLICK_FUSE_THRESHOLD = 0.15;
  private readonly SQUISH_SPEED = 0.015;
  private readonly RELEASE_SPEED = 0.01;

  constructor() {
    this.pointerImage = new Image();
    this.pointerImage.src = '/pointer.svg';
    this.pointerImage.onload = () => { };
  }

  private activeRenderContext: RenderContext | null = null;

  public updateRenderContext(context: RenderContext) {
    this.activeRenderContext = context;
  }

  public startAnimation(renderContext: RenderContext) {
    // console.log('[VideoRenderer] Starting animation');
    this.stopAnimation();
    this.lastDrawTime = 0;
    this.smoothedPositions = null;
    this.activeRenderContext = renderContext;

    const animate = () => {
      // Stop animation loop if video is paused or context missing
      if (!this.activeRenderContext || this.activeRenderContext.video.paused) {
        this.animationFrame = null;
        return;
      }

      const now = performance.now();
      const elapsed = now - this.lastDrawTime;

      if (this.lastDrawTime === 0 || elapsed >= this.FRAME_INTERVAL) {
        this.drawFrame(this.activeRenderContext)
          .catch((err: unknown) => console.error('[VideoRenderer] Draw error:', err));
      }

      this.animationFrame = requestAnimationFrame(animate);
    };

    this.animationFrame = requestAnimationFrame(animate);
  }

  public stopAnimation() {
    if (this.animationFrame !== null) {
      cancelAnimationFrame(this.animationFrame);
      this.animationFrame = null;
      this.lastDrawTime = 0;
      this.activeRenderContext = null;
      this.lastHoldTime = -1;
      this.currentSquishScale = 1.0;
    }
  }

  public drawFrame = async (
    context: RenderContext,
    options: RenderOptions = {}
  ): Promise<void> => {
    if (this.isDrawing) return;

    const { video, canvas, tempCanvas, segment, backgroundConfig, mousePositions } = context;
    if (!video || !canvas || !segment) return;

    // Safety check: video must have data
    if (video.readyState < 2) return;

    const isExportMode = options.exportMode || false;
    const quality = options.highQuality || isExportMode ? 'high' : 'medium';

    const ctx = canvas.getContext('2d', {
      alpha: false,
      willReadFrequently: false
    });
    if (!ctx) return;

    this.isDrawing = true;
    ctx.imageSmoothingQuality = quality as ImageSmoothingQuality;

    const now = performance.now();
    this.latestElapsed = this.lastDrawTime === 0 ? 1000 / 60 : now - this.lastDrawTime;
    this.lastDrawTime = now;

    // Get dimensions from video element
    const vidW = video.videoWidth;
    const vidH = video.videoHeight;

    // Basic safety check for 0-dimension videos
    if (!vidW || !vidH) {
      this.isDrawing = false;
      return;
    }

    const crop = segment.crop || { x: 0, y: 0, width: 1, height: 1 };
    const srcX = vidW * crop.x;
    const srcY = vidH * crop.y;
    const srcW = vidW * crop.width;
    const srcH = vidH * crop.height;

    const canvasW = Math.round(srcW);
    const canvasH = Math.round(srcH);

    // Resize canvas if needed
    if (canvas.width !== canvasW || canvas.height !== canvasH) {
      canvas.width = canvasW;
      canvas.height = canvasH;
      tempCanvas.width = canvasW;
      tempCanvas.height = canvasH;
    }

    if (!isExportMode) {
      canvas.style.aspectRatio = `${canvasW} / ${canvasH}`;
    }

    try {
      const legacyCrop = (backgroundConfig.cropBottom || 0) / 100;
      const scale = backgroundConfig.scale / 100;
      const scaledWidth = canvas.width * scale;
      const scaledHeight = (canvas.height * (1 - legacyCrop)) * scale;
      const x = (canvas.width - scaledWidth) / 2;
      const y = (canvas.height - scaledHeight) / 2;
      const zoomState = this.calculateCurrentZoomState(video.currentTime, segment, canvas.width, canvas.height);

      ctx.save();

      if (zoomState && zoomState.zoomFactor !== 1) {
        const zoomedWidth = canvas.width * zoomState.zoomFactor;
        const zoomedHeight = canvas.height * zoomState.zoomFactor;
        const zoomOffsetX = (canvas.width - zoomedWidth) * zoomState.positionX;
        const zoomOffsetY = (canvas.height - zoomedHeight) * zoomState.positionY;

        ctx.translate(zoomOffsetX, zoomOffsetY);
        ctx.scale(zoomState.zoomFactor, zoomState.zoomFactor);
      }

      // Draw Background
      ctx.fillStyle = this.getBackgroundStyle(
        ctx,
        backgroundConfig.backgroundType,
        backgroundConfig.customBackground
      );
      ctx.fillRect(0, 0, canvas.width, canvas.height);

      // Temp Canvas setup for Rounded Corners
      if (tempCanvas.width !== canvas.width || tempCanvas.height !== canvas.height) {
        tempCanvas.width = canvas.width;
        tempCanvas.height = canvas.height;
      }
      const tempCtx = tempCanvas.getContext('2d', { alpha: true, willReadFrequently: false });
      if (!tempCtx) return;

      tempCtx.clearRect(0, 0, canvas.width, canvas.height);
      tempCtx.save();
      tempCtx.imageSmoothingEnabled = true;
      tempCtx.imageSmoothingQuality = 'high';

      const radius = backgroundConfig.borderRadius;
      const offset = 0.5;

      // Draw Shadow
      if (backgroundConfig.shadow) {
        tempCtx.save();
        tempCtx.shadowColor = 'rgba(0, 0, 0, 0.5)';
        tempCtx.shadowBlur = backgroundConfig.shadow;
        tempCtx.shadowOffsetY = backgroundConfig.shadow * 0.5;

        // Path
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

        tempCtx.fillStyle = '#fff';
        tempCtx.fill();
        tempCtx.restore();
      }

      // Draw Video
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

      tempCtx.clip();

      // Ensure video is drawable
      try {
        tempCtx.drawImage(
          video,
          srcX, srcY, srcW, srcH * (1 - legacyCrop),
          x, y, scaledWidth, scaledHeight
        );
      } catch (e) {
        // Ignore drawImage errors (e.g. if video not fully loaded)
      }

      tempCtx.strokeStyle = 'rgba(0, 0, 0, 0.1)';
      tempCtx.lineWidth = 1;
      tempCtx.stroke();
      tempCtx.restore();

      ctx.drawImage(tempCanvas, 0, 0);

      // Cursor
      const interpolatedPosition = this.interpolateCursorPosition(
        video.currentTime,
        mousePositions
      );
      if (interpolatedPosition) {
        ctx.save();
        ctx.setTransform(1, 0, 0, 1, 0, 0);

        const mX = interpolatedPosition.x;
        const mY = interpolatedPosition.y;

        if (mX >= srcX && mX <= (srcX + srcW) && mY >= srcY && mY <= (srcY + srcH * (1 - legacyCrop))) {
          const relX = (mX - srcX) / srcW;
          const relY = (mY - srcY) / (srcH * (1 - legacyCrop));

          let cursorX = x + (relX * scaledWidth);
          let cursorY = y + (relY * scaledHeight);

          if (zoomState && zoomState.zoomFactor !== 1) {
            cursorX = cursorX * zoomState.zoomFactor + (canvas.width - canvas.width * zoomState.zoomFactor) * zoomState.positionX;
            cursorY = cursorY * zoomState.zoomFactor + (canvas.height - canvas.height * zoomState.zoomFactor) * zoomState.positionY;
          }

          const sizeRatio = Math.min(canvas.width / srcW, canvas.height / srcH);
          const cursorSizeScale = (backgroundConfig.cursorScale || 2) * sizeRatio * (zoomState?.zoomFactor || 1);

          const isActuallyClicked = interpolatedPosition.isClicked;
          const timeSinceLastHold = video.currentTime - this.lastHoldTime;
          const shouldBeSquished = isActuallyClicked || (this.lastHoldTime >= 0 && timeSinceLastHold < this.CLICK_FUSE_THRESHOLD && timeSinceLastHold > 0);

          if (isActuallyClicked) {
            this.lastHoldTime = video.currentTime;
          }

          const targetScale = shouldBeSquished ? 0.75 : 1.0;
          if (this.currentSquishScale > targetScale) {
            this.currentSquishScale = Math.max(targetScale, this.currentSquishScale - this.SQUISH_SPEED * (this.latestElapsed / (1000 / 120)));
          } else if (this.currentSquishScale < targetScale) {
            this.currentSquishScale = Math.min(targetScale, this.currentSquishScale + this.RELEASE_SPEED * (this.latestElapsed / (1000 / 120)));
          }

          this.drawMouseCursor(
            ctx,
            cursorX,
            cursorY,
            shouldBeSquished,
            cursorSizeScale,
            interpolatedPosition.cursor_type || 'default'
          );
        }
        ctx.restore();
      }

      this.backgroundConfig = context.backgroundConfig;

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
    }
  };

  private getBackgroundStyle(
    ctx: CanvasRenderingContext2D,
    type: BackgroundConfig['backgroundType'],
    customBackground?: string
  ): string | CanvasGradient | CanvasPattern {
    switch (type) {
      case 'gradient1': {
        const gradient = ctx.createLinearGradient(0, 0, ctx.canvas.width, 0);
        gradient.addColorStop(0, '#2563eb');
        gradient.addColorStop(1, '#7c3aed');
        return gradient;
      }
      case 'gradient2': {
        const gradient = ctx.createLinearGradient(0, 0, ctx.canvas.width, 0);
        gradient.addColorStop(0, '#fb7185');
        gradient.addColorStop(1, '#fdba74');
        return gradient;
      }
      case 'gradient3': {
        const gradient = ctx.createLinearGradient(0, 0, ctx.canvas.width, 0);
        gradient.addColorStop(0, '#10b981');
        gradient.addColorStop(1, '#2dd4bf');
        return gradient;
      }
      case 'custom': {
        if (customBackground) {
          if (this.lastCustomBackground !== customBackground || !this.customBackgroundPattern) {
            const img = new Image();
            img.src = customBackground;

            if (img.complete) {
              const tempCanvas = document.createElement('canvas');
              const tempCtx = tempCanvas.getContext('2d');

              if (tempCtx) {
                const targetWidth = Math.min(1920, window.innerWidth);
                const scale = targetWidth / img.width;
                const targetHeight = img.height * scale;

                tempCanvas.width = targetWidth;
                tempCanvas.height = targetHeight;
                tempCtx.imageSmoothingEnabled = true;
                tempCtx.imageSmoothingQuality = 'high';
                tempCtx.drawImage(img, 0, 0, targetWidth, targetHeight);
                this.customBackgroundPattern = ctx.createPattern(tempCanvas, 'repeat');
                this.lastCustomBackground = customBackground;
                tempCanvas.remove();
              }
            }
          }

          if (this.customBackgroundPattern) {
            this.customBackgroundPattern.setTransform(new DOMMatrix());
            const scale = Math.max(
              ctx.canvas.width / window.innerWidth,
              ctx.canvas.height / window.innerHeight
            ) * 1.1;
            const matrix = new DOMMatrix().scale(scale);
            this.customBackgroundPattern.setTransform(matrix);
            return this.customBackgroundPattern;
          }
        }
        return '#000000';
      }
      case 'solid': {
        const gradient = ctx.createLinearGradient(0, 0, 0, ctx.canvas.height);
        gradient.addColorStop(0, '#0a0a0a');
        gradient.addColorStop(0.5, '#000000');
        gradient.addColorStop(1, '#0a0a0a');

        const centerX = ctx.canvas.width / 2;
        const centerY = ctx.canvas.height / 2;
        const radialGradient = ctx.createRadialGradient(
          centerX, centerY, 0,
          centerX, centerY, ctx.canvas.width * 0.8
        );
        radialGradient.addColorStop(0, 'rgba(30, 30, 30, 0.15)');
        radialGradient.addColorStop(1, 'rgba(0, 0, 0, 0)');

        ctx.fillStyle = gradient;
        ctx.fillRect(0, 0, ctx.canvas.width, ctx.canvas.height);
        ctx.fillStyle = radialGradient;
        ctx.fillRect(0, 0, ctx.canvas.width, ctx.canvas.height);

        return 'rgba(0,0,0,0)';
      }
      default:
        return '#000000';
    }
  }

  private calculateCurrentZoomState(
    currentTime: number,
    segment: VideoSegment,
    viewW: number,
    viewH: number
  ): ZoomKeyframe {
    const state = this.calculateCurrentZoomStateInternal(currentTime, segment, viewW, viewH);
    this.lastCalculatedState = state;
    return state;
  }

  private calculateCurrentZoomStateInternal(
    currentTime: number,
    segment: VideoSegment,
    viewW: number,
    viewH: number
  ): ZoomKeyframe {
    if (segment.smoothMotionPath && segment.smoothMotionPath.length > 0) {
      const path = segment.smoothMotionPath;
      const idx = path.findIndex((p: any) => p.time >= currentTime);
      let cam = { x: viewW / 2, y: viewH / 2, zoom: 1.0 };

      if (idx === -1) {
        const last = path[path.length - 1];
        cam = { x: last.x, y: last.y, zoom: last.zoom };
      } else if (idx === 0) {
        const first = path[0];
        cam = { x: first.x, y: first.y, zoom: first.zoom };
      } else {
        const p1 = path[idx - 1];
        const p2 = path[idx];
        const t = (currentTime - p1.time) / (p2.time - p1.time);
        cam = {
          x: p1.x + (p2.x - p1.x) * t,
          y: p1.y + (p2.y - p1.y) * t,
          zoom: p1.zoom + (p2.zoom - p1.zoom) * t
        };
      }

      if (segment.zoomInfluencePoints && segment.zoomInfluencePoints.length > 0) {
        const points = segment.zoomInfluencePoints;
        let influence = 1.0;
        const iIdx = points.findIndex((p: { time: number }) => p.time >= currentTime);

        if (iIdx === -1) {
          influence = points[points.length - 1].value;
        } else if (iIdx === 0) {
          influence = points[0].value;
        } else {
          const ip1 = points[iIdx - 1];
          const ip2 = points[iIdx];
          const it = (currentTime - ip1.time) / (ip2.time - ip1.time);
          const cosT = (1 - Math.cos(it * Math.PI)) / 2;
          influence = ip1.value * (1 - cosT) + ip2.value * cosT;
        }

        cam.zoom = 1.0 + (cam.zoom - 1.0) * influence;
        cam.x = (viewW / 2) + (cam.x - (viewW / 2)) * influence;
        cam.y = (viewH / 2) + (cam.y - (viewH / 2)) * influence;
      }

      let resultState: ZoomKeyframe = {
        time: currentTime,
        duration: 0,
        zoomFactor: cam.zoom,
        positionX: cam.x / viewW,
        positionY: cam.y / viewH,
        easingType: 'linear'
      };

      if (segment.zoomKeyframes && segment.zoomKeyframes.length > 0) {
        const WINDOW = 1.5;
        const nearby = segment.zoomKeyframes
          .map((kf: ZoomKeyframe) => ({ kf, dist: Math.abs(kf.time - currentTime) }))
          .filter((item: { kf: ZoomKeyframe; dist: number }) => item.dist < WINDOW)
          .sort((a: { dist: number }, b: { dist: number }) => a.dist - b.dist)[0];

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

    const sortedKeyframes = [...segment.zoomKeyframes].sort((a: ZoomKeyframe, b: ZoomKeyframe) => a.time - b.time);
    if (sortedKeyframes.length === 0) return this.DEFAULT_STATE;

    const nextKeyframe = sortedKeyframes.find(k => k.time > currentTime);
    const prevKeyframe = [...sortedKeyframes].reverse().find(k => k.time <= currentTime);
    const TRANSITION_DURATION = 1.0;

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

    if (prevKeyframe) return prevKeyframe;
    return this.DEFAULT_STATE;
  }

  private easeOutCubic(x: number): number {
    return 1 - Math.pow(1 - x, 3);
  }

  private catmullRomInterpolate(p0: number, p1: number, p2: number, p3: number, t: number): number {
    const t2 = t * t;
    const t3 = t2 * t;
    return 0.5 * (
      (2 * p1) +
      (-p0 + p2) * t +
      (2 * p0 - 5 * p1 + 4 * p2 - p3) * t2 +
      (-p0 + 3 * p1 - 3 * p2 + p3) * t3
    );
  }

  private smoothMousePositions(positions: MousePosition[], targetFps: number = 120): MousePosition[] {
    if (positions.length < 4) return positions;
    const smoothed: MousePosition[] = [];

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
        const cursor_type = t < 0.5 ? p1.cursor_type : p2.cursor_type;
        smoothed.push({ x, y, timestamp, isClicked, cursor_type });
      }
    }

    const windowSize = ((this.backgroundConfig?.cursorSmoothness || 5) * 2) + 1;
    const passes = Math.ceil(windowSize / 2);
    let currentSmoothed = smoothed;

    for (let pass = 0; pass < passes; pass++) {
      const passSmoothed: MousePosition[] = [];
      for (let i = 0; i < currentSmoothed.length; i++) {
        let sumX = 0;
        let sumY = 0;
        let totalWeight = 0;
        const cursor_type = currentSmoothed[i].cursor_type;

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
          cursor_type
        });
      }
      currentSmoothed = passSmoothed;
    }

    const threshold = 0.5 / (windowSize / 2);
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

    if (!this.hasLoggedPositions) {
      this.hasLoggedPositions = true;
    }

    if (!this.smoothedPositions || this.smoothedPositions.length === 0) {
      this.smoothedPositions = this.smoothMousePositions(mousePositions);
    }

    const positions = this.smoothedPositions;
    const exactMatch = positions.find((pos: MousePosition) => Math.abs(pos.timestamp - currentTime) < 0.001);
    if (exactMatch) {
      return {
        x: exactMatch.x,
        y: exactMatch.y,
        isClicked: Boolean(exactMatch.isClicked),
        cursor_type: exactMatch.cursor_type || 'default'
      };
    }

    const nextIndex = positions.findIndex((pos: MousePosition) => pos.timestamp > currentTime);
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
    _isClicked: boolean,
    scale: number = 2,
    cursorType: string
  ) {
    const lowerType = cursorType.toLowerCase();
    ctx.save();
    ctx.translate(x, y);
    ctx.scale(scale, scale);
    ctx.scale(this.currentSquishScale, this.currentSquishScale);

    switch (lowerType) {
      case 'text': {
        ctx.translate(-6, -8);
        const ibeam = new Path2D(`
          M 2 0 L 10 0 L 10 2 L 7 2 L 7 14 L 10 14 L 10 16 L 2 16 L 2 14 L 5 14 L 5 2 L 2 2 Z
        `);
        ctx.strokeStyle = 'white';
        ctx.lineWidth = 1.5;
        ctx.stroke(ibeam);
        ctx.fillStyle = 'black';
        ctx.fill(ibeam);
        break;
      }

      case 'pointer': {
        let imgWidth = 24, imgHeight = 24;
        if (this.pointerImage.complete && this.pointerImage.naturalWidth > 0) {
          imgWidth = this.pointerImage.naturalWidth;
          imgHeight = this.pointerImage.naturalHeight;
        }
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

  private drawTextOverlay(
    ctx: CanvasRenderingContext2D,
    textSegment: TextSegment,
    width: number,
    height: number
  ) {
    ctx.save();
    ctx.font = `${textSegment.style.fontSize}px sans-serif`;
    ctx.fillStyle = textSegment.style.color;
    ctx.textAlign = 'center';

    const x = (textSegment.style.x / 100) * width;
    const y = (textSegment.style.y / 100) * height;

    const metrics = ctx.measureText(textSegment.text);
    const textHeight = textSegment.style.fontSize;
    const hitArea = {
      x: x - metrics.width / 2 - 10,
      y: y - textHeight - 10,
      width: metrics.width + 20,
      height: textHeight + 20
    };

    if (this.draggedTextId === textSegment.id) {
      ctx.fillStyle = 'rgba(0, 121, 211, 0.1)';
      ctx.fillRect(hitArea.x, hitArea.y, hitArea.width, hitArea.height);
    }

    ctx.shadowColor = 'rgba(0,0,0,0.7)';
    ctx.shadowBlur = 6;
    ctx.shadowOffsetX = 2;
    ctx.shadowOffsetY = 2;
    ctx.fillStyle = textSegment.style.color;
    ctx.fillText(textSegment.text, x, y);

    ctx.restore();
    return hitArea;
  }

  public handleMouseDown(e: MouseEvent, segment: VideoSegment, canvas: HTMLCanvasElement) {
    const rect = canvas.getBoundingClientRect();
    const x = (e.clientX - rect.left) * (canvas.width / rect.width);
    const y = (e.clientY - rect.top) * (canvas.height / rect.height);

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

export const videoRenderer = new VideoRenderer();