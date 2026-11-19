'use client';

import { useEffect, useRef, useState } from 'react';
import {
  Camera,
  Cpu,
  FolderLock,
  Globe,
  Package,
  ShieldCheck,
  Smartphone,
  Sparkles,
  Terminal,
} from 'lucide-react';

interface Beat {
  id: string;
  number: string;
  label: string;
  title: string;
  summary: string;
  anchor: number;
}

const beats: readonly Beat[] = [
  {
    id: 'build',
    number: '1',
    label: 'Build and package',
    title: 'Your agent builds a signed .wapk in minutes.',
    summary: 'Describe the app you want. Your coding agent scaffolds, compiles to WebAssembly, and packages showcase.wapk.',
    anchor: 0.14,
  },
  {
    id: 'desktop',
    number: '2',
    label: 'Run and inspect',
    title: 'Drop it into WaOS on desktop with live sandbox controls.',
    summary: 'The app opens in its own GPU-rendered window while WaOS Inspector shows every file, host, and device permission.',
    anchor: 0.52,
  },
  {
    id: 'phone',
    number: '3',
    label: 'Same file on phone',
    title: 'Open the exact same .wapk on your phone.',
    summary: 'No second build and no app store detour. The same WebAssembly component adapts cleanly to a 390 x 844 screen.',
    anchor: 0.9,
  },
];

const inspectorRows = [
  { icon: FolderLock, name: 'Files', detail: '/data private folder', status: 'Scoped', tone: 'blue' },
  { icon: Globe, name: 'Network', detail: 'api.example.com', status: 'Allowed', tone: 'emerald' },
  { icon: Camera, name: 'Camera and Mic', detail: 'Not requested', status: 'Off', tone: 'muted' },
  { icon: Cpu, name: 'Shared runtime', detail: '18 MB RAM, WebGPU', status: 'Shared', tone: 'violet' },
] as const;

function clamp01(value: number): number {
  return Math.max(0, Math.min(1, value));
}

function range(value: number, start: number, end: number): number {
  if (end === start) return value >= end ? 1 : 0;
  return clamp01((value - start) / (end - start));
}

function ease(t: number): number {
  const c = clamp01(t);
  return c * c * (3 - 2 * c);
}

function lerp(a: number, b: number, t: number): number {
  return a + (b - a) * t;
}

/**
 * Sticky scroll-driven choreography of a single `.wapk` file (`showcase.wapk`):
 * Beat 1 (0–33%): agent builds and packages `showcase.wapk`.
 * Beat 2 (33–66%): the `.wapk` tile glides into a desktop window beside an illustrative WaOS Inspector.
 * Beat 3 (66–100%): the same `.wapk` tile lifts out of desktop and glides into a rising 390x844 phone frame.
 * Falls back to a clean clickable 1 / 2 / 3 tabbed view on narrow screens and with reduced motion.
 */
export function WapkStory() {
  const trackRef = useRef<HTMLDivElement>(null);
  const [progress, setProgress] = useState(0);
  const [selectedBeat, setSelectedBeat] = useState(0);
  const [interactiveScroll, setInteractiveScroll] = useState(false);

  useEffect(() => {
    const desktopQuery = matchMedia('(min-width: 768px)');
    const motionQuery = matchMedia('(prefers-reduced-motion: reduce)');
    const updateMode = () => setInteractiveScroll(desktopQuery.matches && !motionQuery.matches);
    updateMode();
    desktopQuery.addEventListener('change', updateMode);
    motionQuery.addEventListener('change', updateMode);
    return () => {
      desktopQuery.removeEventListener('change', updateMode);
      motionQuery.removeEventListener('change', updateMode);
    };
  }, []);

  useEffect(() => {
    if (!interactiveScroll) return;
    let raf = 0;
    const sync = () => {
      raf = 0;
      const track = trackRef.current;
      if (!track) return;
      const rect = track.getBoundingClientRect();
      const navHeight = 56;
      const scrollable = Math.max(1, rect.height - (window.innerHeight - navHeight));
      const next = clamp01((navHeight - rect.top) / scrollable);
      setProgress(next);
    };
    const onScroll = () => {
      if (!raf) raf = requestAnimationFrame(sync);
    };
    sync();
    window.addEventListener('scroll', onScroll, { passive: true });
    window.addEventListener('resize', onScroll, { passive: true });
    return () => {
      if (raf) cancelAnimationFrame(raf);
      window.removeEventListener('scroll', onScroll);
      window.removeEventListener('resize', onScroll);
    };
  }, [interactiveScroll]);

  const activeBeat = interactiveScroll ? (progress < 0.34 ? 0 : progress < 0.67 ? 1 : 2) : selectedBeat;

  const jumpToBeat = (index: number) => {
    setSelectedBeat(index);
    if (!interactiveScroll || !trackRef.current) return;
    const rect = trackRef.current.getBoundingClientRect();
    const navHeight = 56;
    const scrollable = Math.max(1, rect.height - (window.innerHeight - navHeight));
    const top = window.scrollY + rect.top - navHeight + beats[index].anchor * scrollable;
    window.scrollTo({ top, behavior: 'smooth' });
  };

  return (
    <section id="how-it-works" aria-label="How a .wapk file moves from prompt to desktop and phone" className="border-t">
      {interactiveScroll ? (
        <div ref={trackRef} className="relative h-[280svh]">
          <div className="sticky top-14 flex h-[calc(100svh-3.5rem)] flex-col justify-center overflow-hidden">
            <div className="mx-auto flex w-full max-w-[1080px] flex-col gap-6 px-6 py-8">
              <StoryHeader activeBeat={activeBeat} progress={progress} onSelect={jumpToBeat} />
              <DesktopStage progress={progress} />
              <BeatCaption beat={beats[activeBeat]} />
            </div>
          </div>
        </div>
      ) : (
        <div className="mx-auto flex w-full max-w-[1080px] flex-col gap-6 px-6 py-20">
          <StoryHeader activeBeat={activeBeat} progress={beats[activeBeat].anchor} onSelect={jumpToBeat} />
          <StaticFallbackStage beatIndex={activeBeat} />
          <BeatCaption beat={beats[activeBeat]} />
        </div>
      )}
    </section>
  );
}

function StoryHeader({
  activeBeat,
  progress,
  onSelect,
}: {
  activeBeat: number;
  progress: number;
  onSelect: (index: number) => void;
}) {
  return (
    <div className="flex flex-col gap-5 md:flex-row md:items-end md:justify-between">
      <div className="max-w-[540px]">
        <p className="text-sm font-medium text-fd-muted-foreground">One portable .wapk file</p>
        <h2 className="mt-1.5 text-3xl leading-[1.1] font-semibold tracking-tight sm:text-4xl">
          From prompt to desktop and phone.
        </h2>
      </div>
      <div role="tablist" aria-label="Story steps" className="flex flex-wrap gap-2">
        {beats.map((beat, index) => {
          const active = index === activeBeat;
          return (
            <button
              key={beat.id}
              type="button"
              role="tab"
              aria-selected={active}
              onClick={() => onSelect(index)}
              className={`inline-flex items-center gap-2 rounded-full border px-3.5 py-1.5 text-sm font-medium transition-colors ${
                active
                  ? 'border-fd-foreground bg-fd-foreground text-fd-background'
                  : 'border-fd-border bg-fd-card text-fd-muted-foreground hover:text-fd-foreground'
              }`}
            >
              <span
                className={`flex size-5 items-center justify-center rounded-full text-xs font-semibold ${
                  active ? 'bg-fd-background text-fd-foreground' : 'bg-fd-muted text-fd-foreground'
                }`}
              >
                {beat.number}
              </span>
              {beat.label}
            </button>
          );
        })}
      </div>
      <span className="sr-only">{Math.round(progress * 100)}% through story</span>
    </div>
  );
}

function BeatCaption({ beat }: { beat: Beat }) {
  return (
    <div className="flex flex-col gap-1 sm:flex-row sm:items-baseline sm:justify-between sm:gap-6">
      <p className="text-lg font-semibold tracking-tight">{beat.title}</p>
      <p className="max-w-[540px] text-sm leading-relaxed text-fd-muted-foreground sm:text-right">{beat.summary}</p>
    </div>
  );
}

/**
 * Continuous scroll-driven choreography on desktop viewports.
 * All layers animate with GPU transforms and opacity from `progress` in [0, 1].
 */
function DesktopStage({ progress }: { progress: number }) {
  // Beat 1: agent terminal builds showcase.wapk (0 -> 0.36)
  const buildExit = ease(range(progress, 0.27, 0.39));
  const buildOpacity = 1 - buildExit;
  const buildX = lerp(0, -48, buildExit);
  const buildStep1 = ease(range(progress, 0.02, 0.1));
  const buildStep2 = ease(range(progress, 0.1, 0.19));
  const buildStep3 = ease(range(progress, 0.18, 0.27));

  // Beat 2: desktop window + WaOS Inspector (0.31 -> 0.68), then shifts left as phone rises (0.68 -> 0.84)
  const desktopEnter = ease(range(progress, 0.3, 0.44));
  const toPhone = ease(range(progress, 0.66, 0.84));
  const desktopOpacity = desktopEnter * lerp(1, 0.32, toPhone);
  const desktopX = lerp(36, 0, desktopEnter) + lerp(0, -56, toPhone);
  const desktopScale = lerp(0.96, 1, desktopEnter) * lerp(1, 0.92, toPhone);
  const desktopAppLit = ease(range(progress, 0.43, 0.54)) * (1 - ease(range(progress, 0.68, 0.78)));

  // Beat 3: phone frame rises into view on the right (0.66 -> 0.85)
  const phoneEnter = ease(range(progress, 0.66, 0.85));
  const phoneOpacity = phoneEnter;
  const phoneY = lerp(84, 0, phoneEnter);
  const phoneAppLit = ease(range(progress, 0.81, 0.92));

  // The single cohesive `.wapk` tile traveling across all three beats.
  const tileAppear = ease(range(progress, 0.17, 0.27));
  const glideToDesktop = ease(range(progress, 0.32, 0.48));
  const dockInDesktop = ease(range(progress, 0.48, 0.55));
  const liftFromDesktop = ease(range(progress, 0.65, 0.72));
  const glideToPhone = ease(range(progress, 0.69, 0.86));
  const dockInPhone = ease(range(progress, 0.86, 0.93));

  // Percentage coordinates inside the stage box.
  const desktopXPos = 31;
  const desktopYPos = 48;
  const phoneXPos = 68;
  const phoneYPos = 50;

  const tileX =
    progress < 0.65
      ? lerp(64, desktopXPos, glideToDesktop)
      : lerp(desktopXPos, phoneXPos, glideToPhone);

  const arc1 = Math.sin(Math.PI * glideToDesktop) * -10;
  const arc2 = Math.sin(Math.PI * glideToPhone) * -14;
  const tileY =
    progress < 0.65
      ? lerp(52, desktopYPos, glideToDesktop) + arc1
      : lerp(desktopYPos, phoneYPos, glideToPhone) + arc2;

  const tileScale =
    progress < 0.65
      ? lerp(0.82, 1.05, tileAppear) * lerp(1, 0.78, dockInDesktop)
      : lerp(0.78, 1.04, liftFromDesktop) * lerp(1, 0.74, dockInPhone);

  const tileOpacity =
    progress < 0.65
      ? tileAppear * lerp(1, 0.25, dockInDesktop)
      : lerp(0.25, 1, liftFromDesktop) * lerp(1, 0.22, dockInPhone);

  const glowX = lerp(28, 72, progress);

  return (
    <div className="relative h-[460px] w-full overflow-hidden rounded-3xl border bg-fd-card shadow-sm">
      {/* Subtle shifting ambient wash */}
      <div
        aria-hidden
        className="pointer-events-none absolute inset-0 opacity-45 dark:opacity-35"
        style={{
          background: `radial-gradient(520px 320px at ${glowX}% 52%, rgba(99, 102, 241, 0.18), rgba(217, 70, 239, 0.12) 48%, transparent 78%)`,
        }}
      />

      {/* Beat 1: Agent build & package */}
      <div
        className="absolute inset-0 flex items-center justify-center p-8"
        style={{
          opacity: buildOpacity,
          transform: `translate3d(${buildX}px, 0, 0)`,
          pointerEvents: buildOpacity > 0.2 ? 'auto' : 'none',
        }}
      >
        <div className="grid w-full max-w-[820px] grid-cols-12 items-center gap-6">
          <div className="col-span-7 flex flex-col gap-4 rounded-2xl border bg-fd-background p-6 shadow-sm">
            <div className="flex items-center justify-between border-b pb-3">
              <div className="flex items-center gap-2 text-sm font-semibold">
                <Terminal aria-hidden className="size-4 text-blue-500" />
                Coding agent
              </div>
              <span className="rounded-full bg-fd-secondary px-2.5 py-0.5 text-xs font-medium text-fd-muted-foreground">
                WaOS CLI
              </span>
            </div>
            <div className="rounded-xl border bg-fd-card px-4 py-3 text-sm">
              <span className="inline-flex items-center gap-1.5 text-xs font-medium text-fd-muted-foreground">
                <Sparkles aria-hidden className="size-3.5 text-fuchsia-500" />
                Prompt
              </span>
              <p className="mt-1 font-medium">
                &ldquo;Read waos.app/agents.md and build a GPU-rendered showcase app for desktop and phone.&rdquo;
              </p>
            </div>
            <ol className="flex flex-col gap-2 font-mono text-xs">
              <BuildLogRow opacity={buildStep1} command="$ cargo waos new showcase" status="created" />
              <BuildLogRow opacity={buildStep2} command="$ cargo waos build --release" status="wasm32-wasip3" />
              <BuildLogRow opacity={buildStep3} command="$ cargo waos package" status="signed .wapk" highlight />
            </ol>
          </div>
          <div className="col-span-5 flex flex-col items-center justify-center text-center">
            <p className="text-xs font-medium tracking-wide text-fd-muted-foreground uppercase">Output artifact</p>
            <div className="mt-3 h-28 w-44 rounded-2xl border border-dashed border-fd-border/80 bg-fd-background/40" />
            <p className="mt-3 text-xs text-fd-muted-foreground">
              One signed archive ready for macOS and Android
            </p>
          </div>
        </div>
      </div>

      {/* Beat 2: Desktop window + illustrative WaOS Inspector */}
      <div
        className="absolute inset-0 flex items-center justify-center p-6 lg:p-8"
        style={{
          opacity: desktopOpacity,
          transform: `translate3d(${desktopX}px, 0, 0) scale(${desktopScale})`,
          pointerEvents: desktopOpacity > 0.2 ? 'auto' : 'none',
        }}
      >
        <div className="grid h-full max-h-[390px] w-full max-w-[960px] grid-cols-12 gap-5">
          <div className="col-span-7 flex flex-col overflow-hidden rounded-2xl border bg-fd-background shadow-md">
            <div className="flex items-center justify-between border-b bg-fd-card/70 px-4 py-2.5">
              <div className="flex items-center gap-2">
                <span className="size-2.5 rounded-full bg-red-400/80" />
                <span className="size-2.5 rounded-full bg-amber-400/80" />
                <span className="size-2.5 rounded-full bg-emerald-400/80" />
                <span className="ml-2 text-xs font-semibold">Showcase</span>
                <span className="rounded bg-fd-secondary px-1.5 py-0.5 font-mono text-[11px] text-fd-muted-foreground">
                  showcase.wapk
                </span>
              </div>
              <span className="rounded-full border px-2 py-0.5 text-[11px] font-medium text-fd-muted-foreground">
                WebGPU window
              </span>
            </div>
            <div className="relative flex flex-1 flex-col justify-between p-5">
              <ShowcasePlaceholderSurface
                label="Showcase app capture slot"
                dimensions="Desktop window (1280 x 800)"
                active={desktopAppLit}
              />
            </div>
          </div>

          <div className="col-span-5 flex flex-col overflow-hidden rounded-2xl border bg-fd-background shadow-md">
            <div className="flex items-center justify-between border-b bg-fd-card/70 px-4 py-2.5">
              <div className="flex items-center gap-2">
                <ShieldCheck aria-hidden className="size-4 text-blue-500" />
                <span className="text-xs font-semibold">WaOS Inspector</span>
              </div>
              <span className="rounded-full bg-blue-500/10 px-2 py-0.5 text-[11px] font-medium text-blue-600 dark:text-blue-400">
                Illustration
              </span>
            </div>
            <div className="flex flex-1 flex-col justify-between p-4">
              <p className="text-xs leading-relaxed text-fd-muted-foreground">
                Live view of what <code className="text-fd-foreground">showcase.wapk</code> can reach inside its
                sandbox.
              </p>
              <ul className="mt-3 flex flex-col gap-2">
                {inspectorRows.map((row) => (
                  <InspectorRow key={row.name} row={row} lit={desktopAppLit} />
                ))}
              </ul>
            </div>
          </div>
        </div>
      </div>

      {/* Beat 3: Phone frame rising into view */}
      <div
        className="absolute inset-0 flex items-center justify-end pr-8 lg:pr-16"
        style={{
          opacity: phoneOpacity,
          transform: `translate3d(0, ${phoneY}px, 0)`,
          pointerEvents: phoneOpacity > 0.2 ? 'auto' : 'none',
        }}
      >
        <div className="flex items-center gap-8">
          <div className="hidden max-w-[260px] flex-col gap-3 rounded-2xl border bg-fd-background/95 p-5 shadow-md lg:flex">
            <div className="inline-flex w-fit items-center gap-1.5 rounded-full bg-fuchsia-500/10 px-2.5 py-1 text-xs font-medium text-fuchsia-600 dark:text-fuchsia-400">
              <Smartphone aria-hidden className="size-3.5" />
              390 x 844 layout
            </div>
            <p className="text-base font-semibold tracking-tight">Same showcase.wapk, zero rebuild.</p>
            <p className="text-xs leading-relaxed text-fd-muted-foreground">
              The same WebAssembly component opens on Android in its own task and adapts to phone width automatically.
            </p>
          </div>

          {/* Phone hardware frame */}
          <div className="flex h-[404px] w-[214px] flex-col overflow-hidden rounded-[34px] border-[3px] border-fd-foreground/80 bg-fd-background p-2.5 shadow-xl">
            <div className="flex items-center justify-between px-3 pt-1 pb-2 text-[10px] font-semibold text-fd-muted-foreground">
              <span>9:41</span>
              <span className="h-3 w-12 rounded-full bg-fd-foreground/15" />
              <span>5G</span>
            </div>
            <div className="flex items-center justify-between border-b px-2.5 pb-2">
              <span className="text-xs font-semibold">Showcase</span>
              <span className="font-mono text-[10px] text-fd-muted-foreground">390x844</span>
            </div>
            <div className="flex flex-1 flex-col pt-2.5">
              <ShowcasePlaceholderSurface
                label="Showcase capture slot"
                dimensions="Phone layout (390 x 844)"
                active={phoneAppLit}
                compact
              />
            </div>
            <div className="mt-2.5 flex justify-center">
              <span className="h-1 w-16 rounded-full bg-fd-foreground/30" />
            </div>
          </div>
        </div>
      </div>

      {/* Persistent traveling `.wapk` file tile */}
      <div
        aria-hidden
        className="pointer-events-none absolute z-20"
        style={{
          left: `${tileX}%`,
          top: `${tileY}%`,
          opacity: tileOpacity,
          transform: `translate(-50%, -50%) scale(${tileScale})`,
        }}
      >
        <WapkTile />
      </div>

      {/* Progress rail along bottom edge */}
      <div aria-hidden className="absolute inset-x-0 bottom-0 h-1 bg-fd-border/50">
        <div
          className="h-full bg-gradient-to-r from-blue-500 via-fuchsia-500 to-orange-400"
          style={{ width: `${Math.round(progress * 100)}%` }}
        />
      </div>
    </div>
  );
}

/** Static, accessible tabbed stage for mobile screens and `prefers-reduced-motion: reduce`. */
function StaticFallbackStage({ beatIndex }: { beatIndex: number }) {
  if (beatIndex === 0) {
    return (
      <div className="flex flex-col gap-5 rounded-3xl border bg-fd-card p-6">
        <div className="flex items-center justify-between border-b pb-3">
          <div className="flex items-center gap-2 text-sm font-semibold">
            <Terminal aria-hidden className="size-4 text-blue-500" />
            Coding agent
          </div>
          <WapkTile compact />
        </div>
        <div className="rounded-xl border bg-fd-background px-4 py-3 text-sm">
          <p className="font-medium">
            &ldquo;Read waos.app/agents.md and build a GPU-rendered showcase app for desktop and phone.&rdquo;
          </p>
        </div>
        <ol className="flex flex-col gap-2 font-mono text-xs">
          <BuildLogRow opacity={1} command="$ cargo waos new showcase" status="created" />
          <BuildLogRow opacity={1} command="$ cargo waos build --release" status="wasm32-wasip3" />
          <BuildLogRow opacity={1} command="$ cargo waos package" status="signed .wapk" highlight />
        </ol>
      </div>
    );
  }

  if (beatIndex === 1) {
    return (
      <div className="grid gap-5 rounded-3xl border bg-fd-card p-6 md:grid-cols-12">
        <div className="flex flex-col overflow-hidden rounded-2xl border bg-fd-background md:col-span-7">
          <div className="flex items-center justify-between border-b bg-fd-card/70 px-4 py-2.5">
            <span className="text-xs font-semibold">Showcase (showcase.wapk)</span>
            <span className="text-[11px] text-fd-muted-foreground">WebGPU window</span>
          </div>
          <div className="min-h-[220px] p-5">
            <ShowcasePlaceholderSurface
              label="Showcase app capture slot"
              dimensions="Desktop window (1280 x 800)"
              active={1}
            />
          </div>
        </div>
        <div className="flex flex-col overflow-hidden rounded-2xl border bg-fd-background md:col-span-5">
          <div className="flex items-center justify-between border-b bg-fd-card/70 px-4 py-2.5">
            <div className="flex items-center gap-2">
              <ShieldCheck aria-hidden className="size-4 text-blue-500" />
              <span className="text-xs font-semibold">WaOS Inspector</span>
            </div>
            <span className="rounded-full bg-blue-500/10 px-2 py-0.5 text-[11px] font-medium text-blue-600 dark:text-blue-400">
              Illustration
            </span>
          </div>
          <div className="p-4">
            <ul className="flex flex-col gap-2">
              {inspectorRows.map((row) => (
                <InspectorRow key={row.name} row={row} lit={1} />
              ))}
            </ul>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="flex flex-col items-center gap-6 rounded-3xl border bg-fd-card p-6 sm:flex-row sm:justify-center">
      <div className="flex h-[360px] w-[200px] flex-col overflow-hidden rounded-[32px] border-[3px] border-fd-foreground/80 bg-fd-background p-2.5 shadow-lg">
        <div className="flex items-center justify-between border-b px-2.5 pt-1 pb-2 text-xs font-semibold">
          <span>Showcase</span>
          <span className="font-mono text-[10px] text-fd-muted-foreground">390x844</span>
        </div>
        <div className="flex flex-1 flex-col pt-2.5">
          <ShowcasePlaceholderSurface
            label="Showcase capture slot"
            dimensions="Phone layout (390 x 844)"
            active={1}
            compact
          />
        </div>
      </div>
      <div className="max-w-[320px] space-y-3 text-center sm:text-left">
        <WapkTile compact />
        <p className="text-lg font-semibold tracking-tight">Same showcase.wapk, zero rebuild.</p>
        <p className="text-sm leading-relaxed text-fd-muted-foreground">
          The exact same WebAssembly component opens on Android in its own task and adapts cleanly to a 390 x 844 phone
          viewport.
        </p>
      </div>
    </div>
  );
}

function WapkTile({ compact = false }: { compact?: boolean }) {
  if (compact) {
    return (
      <span className="inline-flex items-center gap-2 rounded-xl border bg-fd-background px-3 py-1.5 text-xs font-semibold shadow-sm">
        <span className="flex size-5 items-center justify-center rounded-md bg-gradient-to-br from-blue-500 via-fuchsia-500 to-orange-400 text-white">
          <Package aria-hidden className="size-3" />
        </span>
        showcase.wapk
      </span>
    );
  }
  return (
    <div className="flex items-center gap-3 rounded-2xl border border-fd-foreground/15 bg-fd-background/95 px-4 py-3 shadow-xl backdrop-blur">
      <div className="flex size-11 shrink-0 items-center justify-center rounded-xl bg-gradient-to-br from-blue-500 via-fuchsia-500 to-orange-400 text-white shadow-sm">
        <Package aria-hidden className="size-5" />
      </div>
      <div className="flex flex-col text-left">
        <span className="font-mono text-sm font-semibold">showcase.wapk</span>
        <span className="text-xs text-fd-muted-foreground">Signed WebAssembly app</span>
      </div>
    </div>
  );
}

function BuildLogRow({
  opacity,
  command,
  status,
  highlight = false,
}: {
  opacity: number;
  command: string;
  status: string;
  highlight?: boolean;
}) {
  return (
    <li
      className="flex items-center justify-between gap-2 rounded-lg border bg-fd-card px-3 py-2"
      style={{ opacity, transform: `translate3d(0, ${(1 - opacity) * 8}px, 0)` }}
    >
      <span className="truncate text-fd-foreground">{command}</span>
      <span
        className={`shrink-0 rounded px-1.5 py-0.5 text-[11px] font-medium ${
          highlight
            ? 'bg-emerald-500/15 text-emerald-600 dark:text-emerald-400'
            : 'bg-fd-secondary text-fd-muted-foreground'
        }`}
      >
        {status}
      </span>
    </li>
  );
}

function ShowcasePlaceholderSurface({
  label,
  dimensions,
  active,
  compact = false,
}: {
  label: string;
  dimensions: string;
  active: number;
  compact?: boolean;
}) {
  return (
    <div
      className={`flex h-full flex-col justify-between rounded-xl border border-dashed transition-colors ${
        compact ? 'p-3' : 'p-5'
      }`}
      style={{
        borderColor: active > 0.5 ? 'rgba(99, 102, 241, 0.45)' : undefined,
        background:
          active > 0.2
            ? `linear-gradient(145deg, rgba(59, 130, 246, ${0.09 * active}), rgba(217, 70, 239, ${0.07 * active}) 55%, rgba(251, 146, 60, ${0.07 * active}))`
            : undefined,
      }}
    >
      <div className="flex items-center justify-between gap-2">
        <span className="inline-flex items-center gap-1.5 text-[11px] font-medium text-fd-muted-foreground">
          <span
            className="size-2 rounded-full"
            style={{
              backgroundColor: active > 0.5 ? '#10b981' : 'currentColor',
              opacity: active > 0.5 ? 1 : 0.35,
            }}
          />
          {active > 0.5 ? 'Running in WaOS' : 'Waiting for .wapk'}
        </span>
        {!compact && <span className="font-mono text-[11px] text-fd-muted-foreground">60 fps GPU</span>}
      </div>

      <div className="my-auto py-4 text-center">
        <p className={`font-semibold tracking-tight ${compact ? 'text-xs' : 'text-base'}`}>{label}</p>
        <p className={`mt-1 text-fd-muted-foreground ${compact ? 'text-[11px]' : 'text-xs'}`}>{dimensions}</p>
      </div>

      <div className={`grid gap-2 ${compact ? 'grid-cols-1' : 'grid-cols-3'}`}>
        <div className="h-7 rounded-lg border bg-fd-background/80" />
        {!compact && <div className="h-7 rounded-lg border bg-fd-background/80" />}
        {!compact && <div className="h-7 rounded-lg border bg-fd-background/80" />}
      </div>
    </div>
  );
}

function InspectorRow({
  row,
  lit,
}: {
  row: (typeof inspectorRows)[number];
  lit: number;
}) {
  const Icon = row.icon;
  const badgeClasses =
    row.tone === 'emerald'
      ? 'bg-emerald-500/15 text-emerald-600 dark:text-emerald-400'
      : row.tone === 'blue'
        ? 'bg-blue-500/15 text-blue-600 dark:text-blue-400'
        : row.tone === 'violet'
          ? 'bg-violet-500/15 text-violet-600 dark:text-violet-400'
          : 'bg-fd-secondary text-fd-muted-foreground';

  return (
    <li
      className="flex items-center justify-between gap-2 rounded-xl border bg-fd-card px-3 py-2 text-xs"
      style={{ opacity: lerp(0.55, 1, lit) }}
    >
      <div className="flex min-w-0 items-center gap-2.5">
        <Icon aria-hidden className="size-4 shrink-0 text-fd-muted-foreground" />
        <div className="min-w-0">
          <p className="font-medium text-fd-foreground">{row.name}</p>
          <p className="truncate text-[11px] text-fd-muted-foreground">{row.detail}</p>
        </div>
      </div>
      <span className={`shrink-0 rounded-full px-2 py-0.5 text-[11px] font-medium ${badgeClasses}`}>
        {row.status}
      </span>
    </li>
  );
}
