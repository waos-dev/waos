import Link from 'next/link';
import { Download } from 'lucide-react';
import { buttonVariants } from 'fumadocs-ui/components/ui/button';
import { ServerCodeBlock } from 'fumadocs-ui/components/codeblock.rsc';
import { AgentPromptCard } from '@/components/agent-prompt-card';
import { BuildYourOwn } from '@/components/build-your-own';
import { WaosIcon } from '@/components/waos-icon';
import { WapkStory } from '@/components/wapk-story';
import { WaveStrip } from '@/components/wave-strip';
import { WhyWaos } from '@/components/why-waos';
import { helloCommands } from '@/lib/sample';
import { downloadPath } from '@/lib/releases';
import { agentPrompt, docsRoute, headline, siteOrigin, subtitle } from '@/lib/shared';
import { guideTopics } from '@/lib/source';
import { topicIcon } from '@/lib/topic-icons';

/**
 * Four sections, each with one job: catch the reader in the Hero, walk through one `.wapk` file
 * moving from prompt to desktop and phone, say why an app made for WaOS is worth making, and put
 * agent-first authoring ahead of manual dev docs.
 */
export default function HomePage() {
  return (
    <main className="flex flex-1 flex-col">
      <Hero />
      <WapkStory />
      <WhyWaos />
      <BuildSection />
    </main>
  );
}

/** Catches the reader: the statement, the wave, the downloads, and the agent dialog. */
function Hero() {
  return (
    <section className="relative flex min-h-[calc(100svh-3.5rem)] flex-col items-center overflow-hidden px-6 pt-[14svh] pb-16 text-center">
      <WaveStrip />
      <WaosIcon size={84} className="relative drop-shadow-lg" />
      <h1 className="relative mt-8 text-5xl leading-[1.05] font-semibold tracking-tight sm:text-7xl">
        {headline[0]}
        <br />
        <span className="text-fd-muted-foreground">{headline[1]}</span>
      </h1>
      <p className="relative mt-6 max-w-[640px] text-lg leading-relaxed text-fd-muted-foreground sm:text-xl">
        {subtitle}
      </p>
      <div className="relative mt-9 flex flex-wrap justify-center gap-3">
        <a href={downloadPath('macos')} className={buttonVariants({ color: 'primary', className: 'gap-2 px-6 py-3 text-base' })}>
          <Download aria-hidden className="size-5" strokeWidth={2} />
          Download for macOS
        </a>
        <BuildYourOwn
          prompt={agentPrompt()}
          downloadUrl={downloadPath('macos')}
          androidDownloadUrl={downloadPath('android')}
          startUrl={`${docsRoute}/start`}
        />
      </div>
      <p className="relative mt-6 text-sm text-fd-muted-foreground">
        Available for{' '}
        <a href={downloadPath('macos')} className="font-medium text-fd-foreground underline underline-offset-4">
          macOS (Apple Silicon)
        </a>
        ,{' '}
        <a href={downloadPath('windows')} className="font-medium text-fd-foreground underline underline-offset-4">
          Windows
        </a>{' '}
        and{' '}
        <a href={downloadPath('android')} className="font-medium text-fd-foreground underline underline-offset-4">
          Android
        </a>
        , and for Linux through <code>install.sh</code>.
      </p>
    </section>
  );
}

const docs = [
  { name: 'Start', text: 'Install, make an app, run it, package it.', url: `${docsRoute}/start` },
  { name: 'Concepts', text: 'Programs, views, capabilities, permissions.', url: `${docsRoute}/concepts` },
  { name: 'Waft', text: 'The UI framework apps are written with.', url: `${docsRoute}/waft` },
  { name: 'Reference', text: 'The interfaces, and the SDK on docs.rs.', url: `${docsRoute}/reference` },
];

/** Agent-first authoring up front, with manual CLI commands, docs, and platform capabilities below. */
function BuildSection() {
  return (
    <section className="border-t">
      <div className="mx-auto w-full max-w-[1040px] px-6 py-28">
        <div className="max-w-[780px]">
          <h2 className="text-4xl leading-[1.1] font-semibold tracking-tight sm:text-5xl">
            Build your own with an agent.
          </h2>
          <p className="mt-4 text-lg leading-relaxed text-fd-muted-foreground">
            Hand your coding agent the WaOS playbook and describe what you want. It installs the template, compiles to
            WebAssembly, and opens the running app in WaOS.
          </p>
        </div>

        <div className="mt-10">
          <AgentPromptCard origin={siteOrigin()} />
        </div>

        <div className="mt-20 border-t pt-16">
          <p className="text-sm font-medium text-fd-muted-foreground">Under the hood</p>
          <p className="mt-2 max-w-[720px] text-xl leading-relaxed text-fd-muted-foreground">
            WebAssembly and WebGPU with the whole platform behind them, for anything from a{' '}
            <span className="whitespace-nowrap font-medium text-blue-500">to-do list</span> to a{' '}
            <span className="whitespace-nowrap bg-linear-to-r from-fuchsia-500 to-orange-400 bg-clip-text font-medium text-transparent">
              video editor
            </span>
            .
          </p>
          <div className="mt-10 grid gap-12 lg:grid-cols-2 lg:items-start">
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
