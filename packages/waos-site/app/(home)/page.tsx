import type { ReactNode } from 'react';
import Link from 'next/link';
import { Download } from 'lucide-react';
import { buttonVariants } from 'fumadocs-ui/components/ui/button';
import { ServerCodeBlock } from 'fumadocs-ui/components/codeblock.rsc';
import { BuildYourOwn } from '@/components/build-your-own';
import { WaosIcon } from '@/components/waos-icon';
import { WaveStrip } from '@/components/wave-strip';
import { helloCommands } from '@/lib/sample';
import { agentPrompt, docsRoute, downloadUrl, headline } from '@/lib/shared';
import { guideTopics } from '@/lib/source';
import { topicIcon } from '@/lib/topic-icons';

/**
 * Three sections, each with one job: catch the reader, say what WaOS is and what it gives them,
 * and point whoever builds at the docs.
 */
export default function HomePage() {
  return (
    <main className="flex flex-1 flex-col">
      <Hero />
      <WhatItIs />
      <ForDevelopers />
    </main>
  );
}

/** Catches the reader: the statement, the art, and the two things to do. */
function Hero() {
  return (
    <section className="relative flex min-h-[calc(100svh-3.5rem)] flex-col items-center overflow-hidden px-6 pt-[16svh] text-center">
      <WaveStrip />
      <WaosIcon size={84} className="relative drop-shadow-lg" />
      <h1 className="relative mt-8 text-5xl leading-[1.05] font-semibold tracking-tight sm:text-7xl">
        {headline[0]}
        <br />
        <span className="text-fd-muted-foreground">{headline[1]}</span>
      </h1>
      <div className="relative mt-10 flex flex-wrap justify-center gap-3">
        <a href={downloadUrl} className={buttonVariants({ color: 'primary', className: 'gap-2 px-6 py-3 text-base' })}>
          <Download aria-hidden className="size-5" strokeWidth={2} />
          Download for macOS
        </a>
        <BuildYourOwn prompt={agentPrompt()} downloadUrl={downloadUrl} startUrl={`${docsRoute}/start`} />
      </div>
    </section>
  );
}

/** For the reader who stayed: what WaOS is, in one statement, and what that gives them. */
function WhatItIs() {
  return (
    <section className="border-t">
      <div className="mx-auto w-full max-w-[1040px] px-6 py-28">
        <h2 className="max-w-[820px] text-4xl leading-[1.1] font-semibold tracking-tight sm:text-5xl">
          The home for apps your agent builds.{' '}
          <span className="text-fd-muted-foreground">
            Built on WebAssembly, so an app written minutes ago is safe to run.
          </span>
        </h2>
        <div className="mt-16 grid gap-5 sm:grid-cols-2">
          <Benefit
            title="You grant every permission."
            text="Files, camera, microphone, notifications: an app gets only what you allow."
          >
            <PermissionSwitches />
          </Benefit>
          <Benefit
            title="Light on memory."
            text="Apps share one runtime, where each web-based app brings a browser of its own."
          >
            <SameMemory />
          </Benefit>
          <Benefit
            title="One app, every platform."
            text="The same app file runs wherever WaOS does: macOS, Windows, Linux and Android; iPhone is planned."
          >
            <Devices />
          </Benefit>
          <Benefit
            title="Real apps."
            text="Their own windows, menus and notifications, drawn by the GPU. Not a page in a tab."
          >
            <WindowWithMenus />
          </Benefit>
        </div>
      </div>
    </section>
  );
}

/** One thing WaOS gives, with a small drawing of it above its words. */
function Benefit({ title, text, children }: { title: string; text: string; children: ReactNode }) {
  return (
    <div className="flex flex-col rounded-3xl border bg-fd-card p-8">
      <div aria-hidden className="flex h-44 items-center justify-center">
        {children}
      </div>
      <h3 className="mt-6 text-2xl font-semibold tracking-tight">{title}</h3>
      <p className="mt-2 text-lg leading-relaxed text-fd-muted-foreground">{text}</p>
    </div>
  );
}

/** Three permissions as switches, one granted. */
function PermissionSwitches() {
  const permissions = [
    { name: 'Files', granted: true },
    { name: 'Camera', granted: false },
    { name: 'Microphone', granted: false },
  ];
  return (
    <ul className="flex w-56 flex-col gap-3">
      {permissions.map((permission) => (
        <li key={permission.name} className="flex items-center justify-between rounded-xl border bg-fd-background px-4 py-2.5">
          <span className="font-medium">{permission.name}</span>
          <span
            className={`flex h-6 w-10 items-center rounded-full p-0.5 ${permission.granted ? 'justify-end bg-blue-500' : 'justify-start bg-fd-muted-foreground/30'}`}
          >
            <span className="size-5 rounded-full bg-white shadow-sm" />
          </span>
        </li>
      ))}
    </ul>
  );
}

/** The same memory twice: filled by many WaOS apps, and by a few web-based ones. */
function SameMemory() {
  const memory = 'grid size-28 gap-1 rounded-xl border bg-fd-background p-1.5';
  return (
    <div className="flex gap-8">
      <figure className="flex flex-col items-center gap-2">
        <div className={`${memory} grid-cols-6`}>
          {Array.from({ length: 36 }, (_, app) => (
            <span key={app} className="rounded-[3px] bg-violet-500" />
          ))}
        </div>
        <figcaption className="text-sm font-medium">WaOS apps</figcaption>
      </figure>
      <figure className="flex flex-col items-center gap-2">
        <div className={`${memory} grid-cols-2`}>
          {Array.from({ length: 4 }, (_, app) => (
            <span key={app} className="rounded-md bg-fd-muted-foreground/30" />
          ))}
        </div>
        <figcaption className="text-sm font-medium text-fd-muted-foreground">Web-based apps</figcaption>
      </figure>
    </div>
  );
}

/** The same app, as one coloured tile, on a laptop, a tablet and a phone. */
function Devices() {
  const app = 'rounded-md bg-fuchsia-500';
  return (
    <div className="flex items-end gap-5">
      <div className="flex flex-col items-center">
        <div className="flex h-24 w-36 items-center justify-center rounded-t-xl border-2 border-fd-foreground/70 bg-fd-background">
          <div className={`size-8 ${app}`} />
        </div>
        <div className="h-1.5 w-44 rounded-b-md bg-fd-foreground/70" />
      </div>
      <div className="flex h-20 w-14 items-center justify-center rounded-xl border-2 border-fd-foreground/70 bg-fd-background">
        <div className={`size-6 ${app}`} />
      </div>
      <div className="flex h-14 w-8 items-center justify-center rounded-lg border-2 border-fd-foreground/70 bg-fd-background">
        <div className={`size-4 rounded-sm bg-fuchsia-500`} />
      </div>
    </div>
  );
}

/** An app's own window, under its menu bar, with a notification beside it. */
function WindowWithMenus() {
  return (
    <div className="relative w-60">
      <div className="flex gap-3 px-1 pb-2 text-xs font-medium text-fd-muted-foreground">
        <span className="text-fd-foreground">Notes</span>
        <span>File</span>
        <span>Edit</span>
        <span>View</span>
      </div>
      <div className="h-28 rounded-xl border bg-fd-background shadow-sm">
        <div className="flex gap-1.5 border-b px-3 py-2.5">
          <span className="size-2.5 rounded-full bg-fd-muted-foreground/40" />
          <span className="size-2.5 rounded-full bg-fd-muted-foreground/40" />
          <span className="size-2.5 rounded-full bg-fd-muted-foreground/40" />
        </div>
        <div className="flex flex-col gap-2 p-3">
          <span className="h-2 w-28 rounded-full bg-fd-muted-foreground/30" />
          <span className="h-2 w-20 rounded-full bg-fd-muted-foreground/20" />
        </div>
      </div>
      <div className="absolute -right-6 top-9 flex items-center gap-2 rounded-xl border bg-fd-card px-3 py-2 shadow-md">
        <span className="size-4 rounded-md bg-orange-400" />
        <span className="h-2 w-14 rounded-full bg-fd-muted-foreground/40" />
      </div>
    </div>
  );
}

const docs = [
  { name: 'Start', text: 'Install, make an app, run it, package it.', url: `${docsRoute}/start` },
  { name: 'Concepts', text: 'Programs, views, capabilities, permissions.', url: `${docsRoute}/concepts` },
  { name: 'Waft', text: 'The UI framework apps are written with.', url: `${docsRoute}/frameworks/waft` },
  { name: 'Reference', text: 'The interfaces, and the SDK on docs.rs.', url: `${docsRoute}/reference` },
];

/** For whoever builds: the invitation, what WaOS is technically, then where to read on. */
function ForDevelopers() {
  return (
    <section className="border-t">
      <div className="mx-auto w-full max-w-[1040px] px-6 py-28">
        <h2 className="max-w-[820px] text-4xl leading-[1.1] font-semibold tracking-tight sm:text-5xl">
          Or build one yourself.{' '}
          <span className="text-fd-muted-foreground">
            WebAssembly and WebGPU with the whole platform behind them, for anything from a{' '}
            <span className="whitespace-nowrap text-blue-500">to-do list</span> to a{' '}
            <span className="whitespace-nowrap bg-linear-to-r from-fuchsia-500 to-orange-400 bg-clip-text text-transparent">
              video editor
            </span>
            .
          </span>
        </h2>
        <div className="mt-14 grid gap-12 lg:grid-cols-2 lg:items-start">
          <ServerCodeBlock code={helloCommands} lang="sh" />
          <ul className="grid gap-x-10 gap-y-8 sm:grid-cols-2">
            {docs.map((page) => (
              <li key={page.url}>
                <Link href={page.url} className="text-xl font-semibold underline-offset-4 hover:underline">
                  {page.name} <span aria-hidden>→</span>
                </Link>
                <p className="mt-1 text-fd-muted-foreground">{page.text}</p>
              </li>
            ))}
          </ul>
        </div>
        <Capabilities />
      </div>
    </section>
  );
}

/** What the platform gives an app, each with its sign and its guide, and a word that more is coming. */
function Capabilities() {
  const tile = 'flex size-10 shrink-0 items-center justify-center rounded-xl border bg-fd-card';
  return (
    <div className="mt-24">
      <h3 className="text-2xl font-semibold tracking-tight">Platform capabilities</h3>
      <ul className="mt-8 grid grid-cols-2 gap-x-8 gap-y-5 text-lg sm:grid-cols-3 lg:grid-cols-4">
        {guideTopics().map((topic) => {
          const Icon = topicIcon(topic.url);
          return (
            <li key={topic.url}>
              <Link href={topic.url} className="group flex items-center gap-3">
                <span className={tile}>
                  <Icon aria-hidden className="size-5" strokeWidth={1.75} />
                </span>
                <span className="font-medium underline-offset-4 group-hover:underline">{topic.name}</span>
              </Link>
            </li>
          );
        })}
      </ul>
      <p className="mt-8 text-fd-muted-foreground">More on the way.</p>
    </div>
  );
}
