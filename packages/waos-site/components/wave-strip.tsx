'use client';

import { useEffect, useRef } from 'react';

/**
 * The home page's one piece of art: fine lines bending into a slow wave, drawn on a
 * plain canvas that fills the first section behind its text, the wave low in it. The canvas takes
 * no room of its own, so the text never moves; it is still when the user asks for reduced motion.
 */
export function WaveStrip({ shape = defaultShape }: { shape?: WaveShape }) {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    return canvas ? animateWave(canvas, shape) : undefined;
  }, [shape]);

  return (
    <canvas
      aria-hidden
      ref={canvasRef}
      className="pointer-events-none absolute inset-0 size-full text-fd-foreground [mask-image:linear-gradient(to_right,transparent,black_18%,black_82%,transparent)]"
    />
  );
}

const LINE_WIDTH = 0.75;
/** How far apart, in CSS pixels, the points of a line are. */
const STEP = 4;
/** How long the wave takes to travel one wavelength. */
const PERIOD_MS = 16_000;
/** The wave's shape: how many lines, where its middle is down the canvas, and how far the lines
 * fan out and how high the wave rises, at most, in CSS pixels. */
export interface WaveShape {
  lines: number;
  middle: number;
  spread: number;
  rise: number;
}

const defaultShape: WaveShape = { lines: 90, middle: 0.8, spread: 200, rise: 110 };

/** Draws the wave on `canvas` while it is on screen, until the returned function stops it. */
function animateWave(canvas: HTMLCanvasElement, shape: WaveShape): () => void {
  const context = canvas.getContext('2d');
  if (!context) return () => {};
  const reducedMotion = matchMedia('(prefers-reduced-motion: reduce)');
  const darkScheme = matchMedia('(prefers-color-scheme: dark)');
  let onScreen = true;
  let frame = 0;

  const drawStill = () => drawWave(context, canvas, shape, 0);
  const drawMoving = (time: number) => {
    drawWave(context, canvas, shape, time);
    frame = requestAnimationFrame(drawMoving);
  };
  const restart = () => {
    cancelAnimationFrame(frame);
    if (onScreen) frame = requestAnimationFrame(reducedMotion.matches ? drawStill : drawMoving);
  };

  const resizes = new ResizeObserver(() => {
    fitToDevicePixels(context, canvas);
    restart();
  });
  const visibility = new IntersectionObserver(([entry]) => {
    onScreen = entry.isIntersecting;
    restart();
  });
  resizes.observe(canvas);
  visibility.observe(canvas);
  reducedMotion.addEventListener('change', restart);
  darkScheme.addEventListener('change', restart);

  return () => {
    cancelAnimationFrame(frame);
    resizes.disconnect();
    visibility.disconnect();
    reducedMotion.removeEventListener('change', restart);
    darkScheme.removeEventListener('change', restart);
  };
}

/** Sizes the canvas's pixels to the screen's, so its lines are sharp at any scale. */
function fitToDevicePixels(context: CanvasRenderingContext2D, canvas: HTMLCanvasElement) {
  const ratio = devicePixelRatio;
  canvas.width = Math.round(canvas.clientWidth * ratio);
  canvas.height = Math.round(canvas.clientHeight * ratio);
  context.setTransform(ratio, 0, 0, ratio, 0, 0);
}

/** Draws every line at `time`, in the page's colours. */
function drawWave(
  context: CanvasRenderingContext2D,
  canvas: HTMLCanvasElement,
  shape: WaveShape,
  time: number,
) {
  const width = canvas.clientWidth;
  const height = canvas.clientHeight;
  context.clearRect(0, 0, width, height);
  context.strokeStyle = waveColors(context, width);
  context.lineWidth = LINE_WIDTH;
  const phase = (time / PERIOD_MS) * 2 * Math.PI;
  for (let line = 0; line < shape.lines; line += 1) {
    const offset = (2 * line) / (shape.lines - 1) - 1;
    context.globalAlpha = lineAlpha(offset);
    traceLine(context, width, height, shape, offset, phase);
    context.stroke();
  }
}

/** The page's three colours across the wave, blue through pink to orange. */
function waveColors(context: CanvasRenderingContext2D, width: number): CanvasGradient {
  const colors = context.createLinearGradient(width * 0.15, 0, width * 0.85, 0);
  colors.addColorStop(0, '#3b82f6');
  colors.addColorStop(0.55, '#d946ef');
  colors.addColorStop(1, '#fb923c');
  return colors;
}

/** How strong a line is, by its offset from the middle, -1 to 1: the middle strongest. */
function lineAlpha(offset: number): number {
  return 0.06 + 0.8 * (1 - Math.abs(offset)) ** 2;
}

/** Traces one line across the strip: flat at its ends, bending into the wave in between. */
function traceLine(
  context: CanvasRenderingContext2D,
  width: number,
  height: number,
  shape: WaveShape,
  offset: number,
  phase: number,
) {
  context.beginPath();
  for (let x = 0; x <= width + STEP; x += STEP) {
    const along = x / width;
    const bend = Math.sin(Math.PI * along) ** 2;
    const spread = offset * shape.spread * (0.12 + 0.88 * bend);
    const wave = shape.rise * bend * Math.sin(2 * Math.PI * 1.15 * along - phase + offset * 1.4);
    context.lineTo(x, height * shape.middle + spread + wave);
  }
}
