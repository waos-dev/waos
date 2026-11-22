'use client';

import { useEffect, useRef, useState } from 'react';
import Link from 'next/link';
import { Check, Copy, X } from 'lucide-react';
import { buttonVariants } from 'fumadocs-ui/components/ui/button';
import { useCopyButton } from 'fumadocs-ui/utils/use-copy-button';
import { platformInfo, platformList } from '@/lib/platform';
import { useVisitorPlatform } from '@/lib/use-visitor-platform';

/**
 * The home page's second action: a button naming, one after another, the agents a reader may
 * already use, which opens the three steps to an app of one's own, the middle one the prompt an
 * agent is given, which it copies.
 */
export function BuildYourOwn({
  prompt,
  startUrl,
}: {
  prompt: string;
  startUrl: string;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [copied, copy] = useCopyButton(() => navigator.clipboard.writeText(prompt));
  const [paused, setPaused] = useState(false);
  const shown = useTurns(agents.length, paused);
  const visitor = useVisitorPlatform();
  const primary = visitor === 'other' ? undefined : platformInfo(visitor);
  const orderedPlatforms = primary
    ? [primary, ...platformList.filter((item) => item.id !== primary.id)]
    : platformList;

  return (
    <>
      <button
        type="button"
        aria-label={label}
        onClick={() => dialog.current?.showModal()}
        onPointerEnter={() => setPaused(true)}
        onPointerLeave={() => setPaused(false)}
        className={buttonVariants({ color: 'secondary', className: 'gap-1.5 px-6 py-3 text-base' })}
      >
        Build your app with
        <AgentNames shown={shown} />
      </button>
      <dialog
        ref={dialog}
        aria-labelledby="build-your-own"
        onClick={(event) => {
          if (event.target === dialog.current) dialog.current.close();
        }}
        className="m-auto w-[min(34rem,calc(100vw-2rem))] rounded-3xl border bg-fd-background p-0 text-left text-fd-foreground shadow-2xl backdrop:bg-black/40 backdrop:backdrop-blur-sm"
      >
        <div className="flex flex-col gap-7 p-8">
          <div className="flex items-start justify-between gap-4">
            <h2 id="build-your-own" className="text-2xl font-semibold tracking-tight">
              {label}
            </h2>
            <button
              type="button"
              aria-label="Close"
              onClick={() => dialog.current?.close()}
              className="rounded-full p-1.5 text-fd-muted-foreground hover:bg-fd-accent"
            >
              <X aria-hidden className="size-5" />
            </button>
          </div>
          <ol className="flex flex-col gap-6">
            <Step number={1} title="Get WaOS">
              <div className="flex flex-wrap items-center gap-x-4 gap-y-2 text-sm">
                {orderedPlatforms.map((item) => {
                  const isPrimary = primary?.id === item.id;
                  const label =
                    item.id === 'macos' || isPrimary
                      ? `Download for ${item.name} (${item.detail})`
                      : item.name;
                  return (
                    <a
                      key={item.id}
                      href={item.href}
                      className={
                        isPrimary
                          ? 'font-semibold text-fd-foreground underline underline-offset-4'
                          : 'font-medium text-fd-muted-foreground underline underline-offset-4 hover:text-fd-foreground'
                      }
                    >
                      {label}
                    </a>
                  );
                })}
              </div>
            </Step>
            <Step number={2} title="Give your coding agent this, then say what you want">
              <div className="flex items-center gap-2 rounded-xl border bg-fd-card p-2 pl-4">
                <code className="min-w-0 flex-1 text-sm break-words">{prompt}</code>
                <button
                  type="button"
                  onClick={copy}
                  className={buttonVariants({ color: 'primary', className: 'shrink-0 gap-1.5 px-3 py-2' })}
                >
                  {copied ? <Check aria-hidden className="size-4" /> : <Copy aria-hidden className="size-4" />}
                  {copied ? 'Copied' : 'Copy'}
                </button>
              </div>
            </Step>
            <Step number={3} title="It builds the app and opens it in WaOS" />
          </ol>
          <p className="border-t pt-5 text-fd-muted-foreground">
            Writing it yourself?{' '}
            <Link href={startUrl} className="font-medium text-fd-foreground underline underline-offset-4">
              Start here
            </Link>
          </p>
        </div>
      </dialog>
    </>
  );
}

const label = 'Build your app with an agent';

/** The agents named on the button, each in its maker's colour, or the text's where that is black. */
const agents = [
  { name: 'Claude Code', color: '#d97757' },
  { name: 'Codex', color: '#10a37f' },
  { name: 'Cursor', color: undefined },
  { name: 'Gemini CLI', color: '#4285f4' },
  { name: 'Grok Bot', color: undefined },
];

/** How long each name stays. */
const TURN_MS = 2200;

/** Which of `count` turns it is, moving on every `TURN_MS` unless paused or the user asks for less motion. */
function useTurns(count: number, paused: boolean): number {
  const [turn, setTurn] = useState(0);
  useEffect(() => {
    if (paused || matchMedia('(prefers-reduced-motion: reduce)').matches) return;
    const timer = setInterval(() => setTurn((current) => (current + 1) % count), TURN_MS);
    return () => clearInterval(timer);
  }, [count, paused]);
  return turn;
}

const byLength = [...agents].sort((one, other) => other.name.length - one.name.length);
const longest = byLength[0].name;
const secondLongest = byLength[1].name;

/**
 * The names in one place, so the words before it never move. The place is as wide as the second
 * longest name, which leaves the short ones less room to float in; each sits in its middle, and
 * the longest starts at its left and runs over into the button's own edge.
 */
function AgentNames({ shown }: { shown: number }) {
  const before = (shown + agents.length - 1) % agents.length;
  return (
    <span aria-hidden className="relative inline-block overflow-y-clip font-semibold whitespace-nowrap">
      <span className="invisible">{secondLongest}</span>
      {agents.map((agent, index) => (
        <span
          key={agent.name}
          style={{ color: agent.color }}
          className={`absolute top-0 transition-[translate,opacity] duration-500 ease-out ${
            agent.name === longest ? 'left-0' : 'left-1/2 -translate-x-1/2'
          } ${
            index === shown
              ? 'translate-y-0 opacity-100'
              : index === before
                ? '-translate-y-full opacity-0'
                : 'translate-y-full opacity-0 duration-0'
          }`}
        >
          {agent.name}
        </span>
      ))}
    </span>
  );
}

function Step({ number, title, children }: { number: number; title: string; children?: React.ReactNode }) {
  return (
    <li className="flex gap-4">
      <span className="flex size-7 shrink-0 items-center justify-center rounded-full bg-fd-foreground text-sm font-semibold text-fd-background">
        {number}
      </span>
      <div className="flex min-w-0 flex-1 flex-col gap-2.5 pt-0.5">
        <p className="font-medium">{title}</p>
        {children}
      </div>
    </li>
  );
}
