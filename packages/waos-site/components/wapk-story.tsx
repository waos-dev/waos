'use client';

import { useEffect, useRef, useState, type CSSProperties, type ReactNode } from 'react';
import { AgentSession } from './showcase/agent-session';
import { AndroidPhone } from './showcase/android-phone';
import { AppScreen } from './showcase/breeze-app';
import { InspectorWindow } from './showcase/inspector-window';
import { MacWindow } from './showcase/mac-window';
import { WapkFile } from './showcase/wapk-file';
import { showcase } from './showcase/app';

interface Beat {
  id: string;
  number: string;
  label: string;
  /** The label where the steps share a phone's width. */
  short: string;
  title: string;
  summary: string;
  /** Where the story rests when its step is chosen, from 0 to 1. */
  anchor: number;
}

const beats: readonly Beat[] = [
  {
    id: 'build',
    number: '1',
    label: 'Build and package',
    short: 'Build',
    title: 'Your agent builds the app and packages it.',
    summary:
      'Point your coding agent at the WaOS playbook and describe the app. It scaffolds, writes the code, compiles to WebAssembly and hands you one signed .wapk file.',
    anchor: 0.26,
  },
  {
    id: 'desktop',
    number: '2',
    label: 'Run and inspect',
    short: 'Inspect',
    title: 'Open it in WaOS and see what it does.',
    summary:
      'Breeze runs in its own window. Inspector shows every request, file and device it reaches as it happens, and what WaOS refused.',
    anchor: 0.6,
  },
  {
    id: 'phone',
    number: '3',
    label: 'Same file on phone',
    short: 'Phone',
    title: 'The same file runs on your phone.',
    summary:
      'Open the same .wapk in WaOS on Android. There is no second build: the app lays itself out for a narrow touch screen.',
    anchor: 0.92,
  },
];

/** The drawn stage's own size; it is scaled to fit the space it is given. */
const CANVAS = { width: 1040, height: 480 };
const NAV_HEIGHT = 56;

function clamp01(value: number): number {
  return Math.max(0, Math.min(1, value));
}

function range(value: number, start: number, end: number): number {
  return clamp01((value - start) / (end - start));
}

function ease(t: number): number {
  const c = clamp01(t);
  return c * c * (3 - 2 * c);
}

function lerp(a: number, b: number, t: number): number {
  return a + (b - a) * t;
}

function beatAt(progress: number): number {
  return progress < 0.335 ? 0 : progress < 0.665 ? 1 : 2;
}

/**
 * One `.wapk` file from prompt to desktop and phone, in three steps: an agent builds and packages
 * Breeze; WaOS runs it on a Mac beside Inspector; the same file runs on an Android phone.
 *
 * From `lg` up, with motion allowed, the story is pinned while the page scrolls past it, and the
 * scroll's progress through it, from 0 to 1, draws every layer; a step's button scrolls to where
 * that step rests. Narrower, or with reduced motion, the steps are tabs over still pictures.
 * Each step's layers leave before the next one's come, so no two steps' pictures ever overlap.
 */
export function WapkStory() {
  const trackRef = useRef<HTMLDivElement>(null);
  const [progress, setProgress] = useState(0);
  const [selectedBeat, setSelectedBeat] = useState(0);
  const [pinned, setPinned] = useState(false);

  useEffect(() => {
    const wide = matchMedia('(min-width: 1024px)');
    const still = matchMedia('(prefers-reduced-motion: reduce)');
    const update = () => setPinned(wide.matches && !still.matches);
    update();
    wide.addEventListener('change', update);
    still.addEventListener('change', update);
    return () => {
      wide.removeEventListener('change', update);
      still.removeEventListener('change', update);
    };
  }, []);

  useEffect(() => {
    if (!pinned) return;
    let frame = 0;
    const sync = () => {
      frame = 0;
      const track = trackRef.current;
      if (!track) return;
      const rect = track.getBoundingClientRect();
      const scrollable = Math.max(1, rect.height - (window.innerHeight - NAV_HEIGHT));
      setProgress(clamp01((NAV_HEIGHT - rect.top) / scrollable));
    };
    const onScroll = () => {
      if (!frame) frame = requestAnimationFrame(sync);
    };
    sync();
    window.addEventListener('scroll', onScroll, { passive: true });
    window.addEventListener('resize', onScroll, { passive: true });
    return () => {
      if (frame) cancelAnimationFrame(frame);
      window.removeEventListener('scroll', onScroll);
      window.removeEventListener('resize', onScroll);
    };
  }, [pinned]);

  const activeBeat = pinned ? beatAt(progress) : selectedBeat;

  const choose = (index: number) => {
    setSelectedBeat(index);
    const track = trackRef.current;
    if (!pinned || !track) return;
    const rect = track.getBoundingClientRect();
    const scrollable = Math.max(1, rect.height - (window.innerHeight - NAV_HEIGHT));
    const top = window.scrollY + rect.top - NAV_HEIGHT + beats[index].anchor * scrollable;
    window.scrollTo({ top, behavior: 'smooth' });
  };

  return (
    <section id="how-it-works" aria-label="How a .wapk file moves from prompt to desktop and phone" className="border-t">
      {pinned ? (
        <div ref={trackRef} className="relative h-[300svh]">
          <div className="sticky top-14 h-[calc(100svh-3.5rem)] overflow-hidden">
            <div className="mx-auto flex h-full w-full max-w-[1120px] flex-col justify-center gap-5 px-6 py-6">
              <StoryHeader activeBeat={activeBeat} onSelect={choose} />
              <PinnedStage progress={progress} />
              <BeatCaption beat={beats[activeBeat]} />
            </div>
          </div>
        </div>
      ) : (
        <div className="mx-auto flex w-full max-w-[1120px] flex-col gap-6 px-6 py-20">
          <StoryHeader activeBeat={activeBeat} onSelect={choose} />
          <div role="tabpanel" aria-label={beats[activeBeat].label}>
            <StillStage beat={activeBeat} />
          </div>
          <BeatCaption beat={beats[activeBeat]} />
        </div>
      )}
    </section>
  );
}

function StoryHeader({ activeBeat, onSelect }: { activeBeat: number; onSelect: (index: number) => void }) {
  return (
    <div className="flex flex-col gap-4 xl:flex-row xl:items-end xl:justify-between xl:gap-8">
      <div className="min-w-0">
        <p className="text-sm font-medium text-fd-muted-foreground">One portable .wapk file</p>
        <h2 className="mt-1.5 text-3xl leading-[1.1] font-semibold tracking-tight sm:text-4xl xl:text-[30px] xl:whitespace-nowrap">
          From prompt to desktop and phone.
        </h2>
      </div>
      <div role="tablist" aria-label="Story steps" className="grid shrink-0 grid-cols-3 gap-1.5 sm:flex sm:gap-2">
        {beats.map((beat, index) => {
          const active = index === activeBeat;
          return (
            <button
              key={beat.id}
              type="button"
              role="tab"
              aria-selected={active}
              onClick={() => onSelect(index)}
              className={`inline-flex items-center justify-center gap-2 rounded-full border px-2.5 py-1.5 text-[13px] font-medium whitespace-nowrap transition-colors sm:px-3.5 sm:text-sm ${
                active
                  ? 'border-fd-foreground bg-fd-foreground text-fd-background'
                  : 'border-fd-border bg-fd-card text-fd-muted-foreground hover:text-fd-foreground'
              }`}
            >
              <span
                className={`flex size-5 shrink-0 items-center justify-center rounded-full text-xs font-semibold ${
                  active ? 'bg-fd-background text-fd-foreground' : 'bg-fd-muted text-fd-foreground'
                }`}
              >
                {beat.number}
              </span>
              <span className="sm:hidden">{beat.short}</span>
              <span className="hidden sm:inline">{beat.label}</span>
            </button>
          );
        })}
      </div>
    </div>
  );
}

function BeatCaption({ beat }: { beat: Beat }) {
  return (
    <div className="flex flex-col gap-1.5 xl:flex-row xl:items-baseline xl:justify-between xl:gap-10">
      <p className="shrink-0 text-lg font-semibold tracking-tight">{beat.title}</p>
      <p className="max-w-[560px] text-sm leading-relaxed text-fd-muted-foreground xl:text-right">{beat.summary}</p>
    </div>
  );
}

function StageFrame({ children, className = '' }: { children: ReactNode; className?: string }) {
  return (
    <div
      aria-hidden
      className={`relative overflow-hidden rounded-3xl border bg-fd-card ${className}`}
    >
      {children}
    </div>
  );
}

/** Where a layer sits on the canvas, its opacity and how far it is moved from there. */
function placed(
  box: { x: number; y: number; width: number; height: number },
  opacity: number,
  dx = 0,
  dy = 0,
): CSSProperties {
  return {
    position: 'absolute',
    left: box.x,
    top: box.y,
    width: box.width,
    height: box.height,
    opacity,
    visibility: opacity <= 0.001 ? 'hidden' : 'visible',
    transform: `translate3d(${dx}px, ${dy}px, 0)`,
  };
}

/**
 * The pinned stage: every layer drawn from `progress`, on a canvas of a fixed size scaled to the
 * stage. The steps hand over in turn: the session leaves before the windows come, and Inspector
 * leaves before Breeze's window moves aside for the phone.
 */
function PinnedStage({ progress }: { progress: number }) {
  const frameRef = useRef<HTMLDivElement>(null);
  const [scale, setScale] = useState(1);

  useEffect(() => {
    const frame = frameRef.current;
    if (!frame) return;
    const observer = new ResizeObserver(([entry]) => {
      const { width, height } = entry.contentRect;
      setScale(Math.min(1, width / CANVAS.width, height / CANVAS.height));
    });
    observer.observe(frame);
    return () => observer.disconnect();
  }, []);

  // Step 1: the session, then the package beside it.
  const session = range(progress, 0, 0.24);
  const file = ease(range(progress, 0.2, 0.26));
  const buildLeaves = ease(range(progress, 0.29, 0.335));

  // Step 2: Breeze's window and Inspector, whose events come in one by one.
  const breezeEnters = ease(range(progress, 0.335, 0.41));
  const inspectorEnters = ease(range(progress, 0.37, 0.44));
  const rows = [0.44, 0.46, 0.48, 0.5, 0.52, 0.54].filter((at) => progress >= at).length;
  const detail = progress >= 0.565;
  const inspectorLeaves = ease(range(progress, 0.63, 0.67));

  // Step 3: Breeze's window moves aside and the phone rises beside it.
  const breezeMoves = ease(range(progress, 0.67, 0.78));
  const phoneEnters = ease(range(progress, 0.71, 0.82));

  return (
    <StageFrame className="max-h-[520px] min-h-[300px] flex-1">
      <div ref={frameRef} className="absolute inset-0 flex items-center justify-center">
        <div
          className="relative shrink-0"
          style={{ width: CANVAS.width, height: CANVAS.height, transform: `scale(${scale})` }}
        >
          <div style={placed({ x: 100, y: 20, width: 600, height: 440 }, 1 - buildLeaves, 0, -14 * buildLeaves)}>
            <AgentSession progress={session} className="size-full" />
          </div>
          <div
            className="flex items-center"
            style={placed({ x: 750, y: 20, width: 190, height: 440 }, file * (1 - buildLeaves), lerp(-28, 0, file), -14 * buildLeaves)}
          >
            <WapkFile />
          </div>

          <div
            style={placed(
              { x: lerp(16, 166, breezeMoves), y: 40, width: 420, height: 400 },
              breezeEnters,
              0,
              lerp(16, 0, breezeEnters),
            )}
          >
            <MacWindow title={showcase.name} className="size-full">
              <AppScreen layout="desktop" />
            </MacWindow>
          </div>
          <div
            style={placed(
              { x: 452, y: 14, width: 572, height: 452 },
              inspectorEnters * (1 - inspectorLeaves),
              24 * inspectorLeaves,
              lerp(16, 0, inspectorEnters),
            )}
          >
            <InspectorWindow rows={rows} detail={detail} className="size-full" />
          </div>

          <div style={placed({ x: 658, y: 18, width: 214, height: 445 }, phoneEnters, 0, lerp(70, 0, phoneEnters))}>
            <AndroidPhone width={214} time="10:44">
              <AppScreen layout="phone" />
            </AndroidPhone>
          </div>
        </div>
      </div>
      <div className="absolute inset-x-0 bottom-0 h-[3px] bg-fd-border/40">
        <div className="h-full bg-fd-foreground/25" style={{ width: `${progress * 100}%` }} />
      </div>
    </StageFrame>
  );
}

/** A still picture of one step, for narrow screens and reduced motion. */
function StillStage({ beat }: { beat: number }) {
  if (beat === 0) {
    return (
      <StageFrame className="flex flex-col items-center gap-8 p-4 sm:p-8 md:flex-row md:justify-center">
        <AgentSession progress={1} className="w-full max-w-[600px] md:h-[440px]" />
        <WapkFile />
      </StageFrame>
    );
  }
  if (beat === 1) {
    return (
      <StageFrame className="grid gap-5 p-4 sm:p-6 md:grid-cols-[minmax(0,5fr)_minmax(0,7fr)] md:items-center">
        <MacWindow title={showcase.name} className="h-[400px]">
          <AppScreen layout="desktop" />
        </MacWindow>
        <InspectorWindow rows={6} detail className="h-[452px]" />
      </StageFrame>
    );
  }
  return (
    <StageFrame className="flex items-center justify-center gap-14 px-4 py-8 sm:px-8">
      <MacWindow title={showcase.name} className="hidden h-[400px] w-[420px] md:flex">
        <AppScreen layout="desktop" />
      </MacWindow>
      <AndroidPhone width={214} time="10:44">
        <AppScreen layout="phone" />
      </AndroidPhone>
    </StageFrame>
  );
}
