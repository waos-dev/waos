import Link from 'next/link';
import type { ReactNode } from 'react';
import {
  AppWindow,
  Camera,
  CirclePlay,
  FolderOpen,
  Globe,
  Mic,
  Network,
  type LucideIcon,
} from 'lucide-react';
import { docsRoute } from '@/lib/shared';
import { guideTopics } from '@/lib/source';

/**
 * Why WaOS apps are built this way, as a 2×2 asymmetric bento: each card a title, two lines at
 * most, and one quiet picture of the claim at its bottom. On wide screens the cards of a row share
 * a subgrid, so their pictures start on one line and end on another; keep each card's text short
 * enough that this stays true.
 */
export function WhyWaos() {
  return (
    <section id="why-waos" aria-labelledby="why-waos-title" className="border-t">
      <div className="mx-auto w-full max-w-[1120px] px-6 py-24 sm:py-28">
        <div className="max-w-[720px]">
          <p className="text-sm font-medium text-fd-muted-foreground">Why WaOS apps</p>
          <h2 id="why-waos-title" className="mt-2 text-4xl leading-[1.08] font-semibold tracking-tight sm:text-5xl">
            Built like native. Packaged like a file.
          </h2>
        </div>

        <div className="mt-12 grid gap-4 lg:grid-cols-12">
          <BentoCard
            id="light"
            span="lg:col-span-5"
            title="Fast, light on memory and CPU"
            text="Rust compiled to WebAssembly, sharing one GPU runtime. No browser engine bundled per app."
          >
            <Footprint />
          </BentoCard>
          <BentoCard
            id="native"
            span="lg:col-span-7"
            title="Write once, native on desktop and phone"
            text="One .wapk runs on macOS, Windows, Linux, and Android with Waft's Fluent and Cupertino kits, real windows, system menus, and touch gestures."
          >
            <DesktopAndPhone />
          </BentoCard>
          <BentoCard
            id="capable"
            span="lg:col-span-7"
            title="Rich system APIs out of the box"
            text={`${guideTopics().length} typed capability guides from WebGPU and media to files and local processes\u00a0— enough for a daily utility or a video editor.`}
          >
            <CapabilityGroups />
          </BentoCard>
          <BentoCard
            id="control"
            span="lg:col-span-5"
            title="Everything under your control"
            text="Per-app permissions, domain and folder scoping, simulated devices, and live inspection built into WaOS."
          >
            <Permissions />
          </BentoCard>
        </div>
      </div>
    </section>
  );
}

function BentoCard({
  id,
  span,
  title,
  text,
  children,
}: {
  id: string;
  span: string;
  title: string;
  text: string;
  children: ReactNode;
}) {
  return (
    <div
      id={`why-${id}`}
      className={`flex flex-col gap-6 rounded-2xl border bg-fd-card p-5 sm:p-7 lg:row-span-2 lg:grid lg:grid-rows-subgrid ${span}`}
    >
      <div>
        <h3 className="text-xl leading-snug font-semibold tracking-tight">{title}</h3>
        <p className="mt-2 text-[15px] leading-relaxed text-pretty text-fd-muted-foreground">{text}</p>
      </div>
      {children}
    </div>
  );
}

/** The elevated inner card every bento card ends with. */
function Panel({ children, className = '' }: { children: ReactNode; className?: string }) {
  return (
    <div className={`flex flex-col rounded-xl border bg-fd-popover shadow-sm shadow-black/[0.03] ${className}`}>
      {children}
    </div>
  );
}

function PanelFooter({ children }: { children: ReactNode }) {
  return <div className="mt-auto border-t px-4 py-3 text-xs text-fd-muted-foreground sm:px-5">{children}</div>;
}

/** Three apps open: an engine in each, against one runtime they share. No numbers, only shape. */
function Footprint() {
  return (
    <Panel>
      <div className="flex flex-1 flex-col justify-center gap-6 px-4 py-5 sm:px-5">
        <FootprintRow label="Web-wrapped apps" count="3 apps, 3 engines" note="Browser engine × each app">
          {[0, 1, 2].map((app) => (
            <span key={app} className="flex h-full flex-1 overflow-hidden rounded-[3px] bg-amber-400/35 dark:bg-amber-500/30">
              <span className="h-full w-[18%] bg-amber-500/70 dark:bg-amber-400" />
            </span>
          ))}
        </FootprintRow>
        <FootprintRow label="WaOS apps" count="3 apps, 1 runtime" note="One shared WaOS runtime + lean Wasm">
          <span className="h-full w-[22%] rounded-[3px] bg-linear-to-r from-blue-500 to-violet-500" />
          {[0, 1, 2].map((app) => (
            <span key={app} className="h-full w-[4%] rounded-[3px] bg-blue-400/70" />
          ))}
        </FootprintRow>
      </div>
      <PanelFooter>Rust + WebAssembly on one shared GPU runtime</PanelFooter>
    </Panel>
  );
}

function FootprintRow({
  label,
  count,
  note,
  children,
}: {
  label: string;
  count: string;
  note: string;
  children: ReactNode;
}) {
  return (
    <div>
      <p className="flex items-baseline justify-between gap-3">
        <span className="text-sm font-medium">{label}</span>
        <span className="text-xs text-fd-muted-foreground">{count}</span>
      </p>
      <div aria-hidden className="mt-2 flex h-3.5 gap-[3px] rounded-[4px] bg-fd-secondary/70">
        {children}
      </div>
      <p className="mt-1.5 text-xs text-fd-muted-foreground">{note}</p>
    </div>
  );
}

/** The same app as a small desktop window beside a small phone. */
function DesktopAndPhone() {
  return (
    <Panel className="justify-center">
      <div aria-hidden className="flex items-end gap-3 p-4 sm:gap-5 sm:p-5">
        <figure className="min-w-0 flex-1">
          <div className="overflow-hidden rounded-lg border bg-fd-background">
            <div className="flex items-center gap-1.5 border-b px-3 py-2">
              <span className="size-2 rounded-full bg-red-400/80" />
              <span className="size-2 rounded-full bg-amber-400/80" />
              <span className="size-2 rounded-full bg-green-400/80" />
              <span className="ml-auto text-[11px] text-fd-muted-foreground">Notes</span>
            </div>
            <div className="flex h-[152px]">
              <ul className="hidden w-24 shrink-0 flex-col gap-0.5 border-r p-2 text-xs sm:flex">
                <li className="rounded-md bg-fd-secondary px-2 py-1 font-medium">Notes</li>
                <li className="px-2 py-1 text-fd-muted-foreground">Drafts</li>
                <li className="px-2 py-1 text-fd-muted-foreground">Archive</li>
              </ul>
              <div className="flex min-w-0 flex-1 flex-col p-3">
                <p className="text-sm font-semibold">Release checklist</p>
                <p className="mt-1 text-xs leading-snug text-fd-muted-foreground">
                  Real windows, menus, and keyboard shortcuts
                </p>
                <div className="mt-3 hidden flex-col gap-1.5 sm:flex">
                  <span className="h-1.5 w-11/12 rounded-full bg-fd-secondary" />
                  <span className="h-1.5 w-2/3 rounded-full bg-fd-secondary" />
                </div>
                <div className="mt-auto flex gap-1.5">
                  <Keycap>⌘S</Keycap>
                  <Keycap>⌘N</Keycap>
                </div>
              </div>
            </div>
          </div>
          <figcaption className="mt-2 text-center text-[11px] whitespace-nowrap text-fd-muted-foreground sm:text-xs">macOS — Fluent</figcaption>
        </figure>

        <figure className="w-[112px] shrink-0 sm:w-[136px]">
          <div className="flex h-[200px] flex-col rounded-[20px] border-[3px] border-fd-foreground/80 bg-fd-background px-2 pt-1.5 pb-1.5">
            <span className="mx-auto size-1.5 rounded-full bg-fd-foreground/70" />
            <p className="mt-1.5 text-[11px] font-medium text-blue-500">‹ Notes</p>
            <div className="mt-2 rounded-lg bg-fd-secondary p-2">
              <p className="text-[11px] leading-tight font-semibold">Release checklist</p>
              <p className="mt-1 text-[10px] leading-snug text-fd-muted-foreground">Swipe to go back</p>
            </div>
            <div className="mt-1.5 rounded-lg bg-fd-secondary/60 p-2">
              <span className="block h-1.5 w-4/5 rounded-full bg-fd-foreground/10" />
              <span className="mt-1.5 block h-1.5 w-3/5 rounded-full bg-fd-foreground/10" />
            </div>
            <span className="mx-auto mt-auto h-1 w-10 rounded-full bg-fd-foreground/30" />
          </div>
          <figcaption className="mt-2 text-center text-[11px] whitespace-nowrap text-fd-muted-foreground sm:text-xs">Android — Cupertino</figcaption>
        </figure>
      </div>
    </Panel>
  );
}

function Keycap({ children }: { children: ReactNode }) {
  return (
    <kbd className="rounded-md border border-b-2 bg-fd-popover px-1.5 py-0.5 font-sans text-[11px] font-medium text-fd-muted-foreground">
      {children}
    </kbd>
  );
}

const capabilityGroups: readonly { label: string; icon: LucideIcon; tint: string; slugs: readonly string[] }[] = [
  {
    label: 'Graphics & UI',
    icon: AppWindow,
    tint: 'bg-blue-500/10 text-blue-600 dark:text-blue-400',
    slugs: ['webgpu', 'windows', 'compositing', 'system-bars'],
  },
  {
    label: 'Media',
    icon: CirclePlay,
    tint: 'bg-fuchsia-500/10 text-fuchsia-600 dark:text-fuchsia-400',
    slugs: ['cameras', 'audio', 'media-players', 'images'],
  },
  {
    label: 'Files & system',
    icon: FolderOpen,
    tint: 'bg-amber-500/10 text-amber-600 dark:text-amber-400',
    slugs: ['file-dialogs', 'file-watching', 'clipboard', 'processes'],
  },
  {
    label: 'Connected',
    icon: Network,
    tint: 'bg-emerald-500/10 text-emerald-600 dark:text-emerald-400',
    slugs: ['network', 'notifications', 'menus', 'workers'],
  },
];

/** A few guides by what they let an app do, each linked, and the way to the rest. */
function CapabilityGroups() {
  const topics = guideTopics();
  const topic = (slug: string) => topics.find((item) => item.url === `${docsRoute}/guide/${slug}`);
  return (
    <Panel>
      <ul className="divide-y">
        {capabilityGroups.map((group) => {
          const Icon = group.icon;
          return (
            <li key={group.label} className="flex flex-col gap-2 px-4 py-3 sm:flex-row sm:items-center sm:gap-4 sm:px-5">
              <span className="flex shrink-0 items-center gap-2.5 sm:w-36 text-sm font-medium">
                <span className={`flex size-6 items-center justify-center rounded-md ${group.tint}`}>
                  <Icon aria-hidden className="size-3.5" strokeWidth={2} />
                </span>
                {group.label}
              </span>
              <span className="flex flex-wrap gap-1.5">
                {group.slugs.map((slug) => {
                  const page = topic(slug);
                  if (!page) return null;
                  return (
                    <Link
                      key={slug}
                      href={page.url}
                      className="rounded-md border bg-fd-background px-2 py-0.5 text-xs font-medium whitespace-nowrap transition-colors hover:bg-fd-accent"
                    >
                      {page.name}
                    </Link>
                  );
                })}
              </span>
            </li>
          );
        })}
      </ul>
      <div className="mt-auto border-t px-4 py-3 sm:px-5">
        <Link
          href={`${docsRoute}/guide`}
          className="text-sm font-medium underline-offset-4 hover:underline"
        >
          Browse all {topics.length} capability guides <span aria-hidden>→</span>
        </Link>
      </div>
    </Panel>
  );
}

const permissions: readonly { name: string; icon: LucideIcon; value: string; tint: string }[] = [
  {
    name: 'Network',
    icon: Globe,
    value: 'api.weather.gov',
    tint: 'bg-emerald-500/10 text-emerald-700 dark:text-emerald-400',
  },
  {
    name: 'Folders',
    icon: FolderOpen,
    value: '~/Documents/Notes',
    tint: 'bg-blue-500/10 text-blue-700 dark:text-blue-400',
  },
  {
    name: 'Camera',
    icon: Camera,
    value: 'Ask on first use',
    tint: 'bg-amber-500/10 text-amber-700 dark:text-amber-400',
  },
  {
    name: 'Microphone',
    icon: Mic,
    value: 'Simulated audio',
    tint: 'bg-violet-500/10 text-violet-700 dark:text-violet-400',
  },
];

/** One app's permissions as WaOS keeps them, a row each. */
function Permissions() {
  return (
    <Panel>
      <ul className="flex flex-1 flex-col divide-y">
        {permissions.map((permission) => {
          const Icon = permission.icon;
          return (
            <li key={permission.name} className="flex flex-1 items-center justify-between gap-3 px-4 py-3 sm:px-5">
              <span className="flex items-center gap-2.5 text-sm font-medium">
                <Icon aria-hidden className="size-4 text-fd-muted-foreground" strokeWidth={1.75} />
                {permission.name}
              </span>
              <span className={`rounded-full px-2.5 py-0.5 text-xs font-medium whitespace-nowrap ${permission.tint}`}>
                {permission.value}
              </span>
            </li>
          );
        })}
      </ul>
      <PanelFooter>
        <span className="inline-flex items-center gap-2">
          <span className="size-1.5 rounded-full bg-emerald-500" />
          Every use shows live in Inspector
        </span>
      </PanelFooter>
    </Panel>
  );
}
