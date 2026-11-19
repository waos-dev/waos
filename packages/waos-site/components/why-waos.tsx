import Link from 'next/link';
import { Blocks, Cpu, MonitorSmartphone, ShieldCheck, type LucideIcon } from 'lucide-react';
import { docsRoute } from '@/lib/shared';
import { guideTopics } from '@/lib/source';

interface Pillar {
  id: string;
  span: string;
  icon: LucideIcon;
  title: string;
  text: string;
  highlights?: readonly string[];
  link?: { label: string; href: string };
}

function pillars(): readonly Pillar[] {
  return [
    {
      id: 'light',
      span: 'lg:col-span-5',
      icon: Cpu,
      title: 'Fast and light on your machine',
      text: 'Written in Rust and compiled to WebAssembly. Every app shares one GPU-accelerated runtime instead of bundling its own browser engine, keeping memory low and CPU quiet.',
      highlights: ['Rust and WebAssembly', 'Shared GPU runtime'],
    },
    {
      id: 'native',
      span: 'lg:col-span-7',
      icon: MonitorSmartphone,
      title: 'Write once, run natively everywhere',
      text: 'One .wapk runs across desktop and phone with real windows, system menus, keyboard shortcuts, and responsive touch ergonomics through Waft. No per-platform rewrites, and no web view.',
      highlights: ['macOS (Apple Silicon)', 'Android', 'Waft Fluent and Cupertino'],
    },
    {
      id: 'capable',
      span: 'lg:col-span-7',
      icon: Blocks,
      title: 'Rich system APIs out of the box',
      text: 'WebGPU graphics, cameras, audio, media playback, file dialogs, networking, menus, notifications, local processes, and background workers — typed capabilities ready for anything from a daily utility to a video editor.',
      link: {
        label: `Explore all ${guideTopics().length} capability guides`,
        href: `${docsRoute}/guide`,
      },
    },
    {
      id: 'control',
      span: 'lg:col-span-5',
      icon: ShieldCheck,
      title: 'Everything under your control',
      text: 'Apps reach only the folders and domains you allow, ask before first using your camera, microphone, or notifications, and can even be given simulated devices — with every use visible live in Inspector.',
      highlights: ['Per-app permissions', 'Simulated devices', 'Live Inspector'],
    },
  ];
}

/**
 * Why WaOS apps are built this way, in four simple asymmetric bento blocks:
 * low memory and CPU on a shared runtime, one native binary across platforms,
 * rich typed system APIs, and per-app permissions with live inspection.
 */
export function WhyWaos() {
  return (
    <section id="why-waos" aria-labelledby="why-waos-title" className="border-t">
      <div className="mx-auto w-full max-w-[1040px] px-6 py-24 sm:py-28">
        <div className="max-w-[720px]">
          <p className="text-sm font-medium text-fd-muted-foreground">Why WaOS apps</p>
          <h2 id="why-waos-title" className="mt-2 text-4xl leading-[1.08] font-semibold tracking-tight sm:text-5xl">
            Built like native. Packaged like a file.
          </h2>
        </div>

        <div className="mt-12 grid gap-4 lg:grid-cols-12">
          {pillars().map((pillar) => {
            const Icon = pillar.icon;
            return (
              <div
                key={pillar.id}
                id={`why-${pillar.id}`}
                className={`flex flex-col justify-between gap-8 rounded-2xl border bg-fd-card p-7 sm:p-8 ${pillar.span}`}
              >
                <div>
                  <span className="inline-flex size-10 items-center justify-center rounded-xl border bg-fd-secondary/60 text-fd-foreground">
                    <Icon aria-hidden className="size-5" strokeWidth={1.75} />
                  </span>
                  <h3 className="mt-5 text-xl leading-snug font-semibold tracking-tight sm:text-2xl">{pillar.title}</h3>
                  <p className="mt-3 text-base leading-relaxed text-pretty text-fd-muted-foreground">{pillar.text}</p>
                </div>

                {pillar.highlights ? (
                  <div className="flex flex-wrap gap-2">
                    {pillar.highlights.map((item) => (
                      <span
                        key={item}
                        className="rounded-lg border bg-fd-secondary/40 px-2.5 py-1 text-xs font-medium text-fd-muted-foreground"
                      >
                        {item}
                      </span>
                    ))}
                  </div>
                ) : pillar.link ? (
                  <div>
                    <Link
                      href={pillar.link.href}
                      className="inline-flex items-center gap-1.5 text-sm font-medium text-fd-foreground underline-offset-4 hover:underline"
                    >
                      {pillar.link.label} <span aria-hidden>→</span>
                    </Link>
                  </div>
                ) : null}
              </div>
            );
          })}
        </div>
      </div>
    </section>
  );
}
