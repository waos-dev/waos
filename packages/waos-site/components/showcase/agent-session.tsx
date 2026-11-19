import type { ReactNode } from 'react';
import { showcase } from './app';
import { MacWindow } from './mac-window';

/** One line of the session and the point of the story, from 0 to 1, where it shows. */
interface Line {
  at: number;
  body: ReactNode;
  /** A gap above it, as the agent leaves between its steps. */
  spaced?: boolean;
}

const dim = 'rgba(255,255,255,0.46)';
const text = 'rgba(255,255,255,0.88)';
const green = '#4ade80';
const cargoGreen = '#22c55e';

function Tool({ name, arg }: { name: string; arg: string }) {
  return (
    <span className="flex gap-2">
      <span style={{ color: green }}>●</span>
      <span className="min-w-0">
        <span className="font-semibold" style={{ color: text }}>
          {name}
        </span>
        <span style={{ color: dim }}>({arg})</span>
      </span>
    </span>
  );
}

function Result({ children }: { children: ReactNode }) {
  return (
    <span className="flex gap-2 pl-[2px]">
      <span style={{ color: dim }}>└</span>
      <span className="min-w-0" style={{ color: dim }}>
        {children}
      </span>
    </span>
  );
}

function Continued({ children }: { children: ReactNode }) {
  return (
    <span className="block pl-[22px] break-words whitespace-pre-wrap" style={{ color: dim }}>
      {children}
    </span>
  );
}

const lines: readonly Line[] = [
  { at: 0.04, spaced: true, body: <Tool name="Fetch" arg="https://waos.app/agents.md" /> },
  { at: 0.1, body: <Result>Received 9.6 KB (200 OK)</Result> },
  { at: 0.18, body: <Tool name="Bash" arg="cargo waos new breeze" /> },
  { at: 0.24, body: <Result>created breeze</Result> },
  { at: 0.32, body: <Tool name="Update" arg="src/main.rs" /> },
  { at: 0.38, body: <Result>Added the forecast, the cities and the hourly strip</Result> },
  { at: 0.46, body: <Tool name="Bash" arg="cargo waos package" /> },
  {
    at: 0.54,
    body: (
      <Result>
        <span className="whitespace-pre">
          <span className="font-semibold" style={{ color: cargoGreen }}>
            Compiling
          </span>{' '}
          breeze v0.1.0
        </span>
      </Result>
    ),
  },
  {
    at: 0.64,
    body: (
      <Continued>
        <span className="font-semibold" style={{ color: cargoGreen }}>
          Finished
        </span>{' '}
        `release` profile [optimized] target(s) in 41.28s
      </Continued>
    ),
  },
  { at: 0.7, body: <Continued>cargo waos: listed in target/waos/index.json</Continued> },
  {
    at: 0.74,
    body: (
      <Continued>
        <span style={{ color: text }}>target/waos/{showcase.file}</span>
      </Continued>
    ),
  },
  {
    at: 0.84,
    spaced: true,
    body: (
      <span className="flex gap-2">
        <span style={{ color: text }}>●</span>
        <span className="min-w-0" style={{ color: text }}>
          Breeze is packaged. Open {showcase.file} in WaOS on your Mac or your Android phone.
        </span>
      </span>
    ),
  },
];

function shown(progress: number, at: number): number {
  return Math.max(0, Math.min(1, (progress - at) / 0.06));
}

/**
 * A coding agent's session in a terminal, as Claude Code, Codex or OpenCode show one: the
 * reader's prompt, then each tool the agent calls with what it answered, ending in the package.
 * `progress` from 0 to 1 shows its lines one by one; every line keeps its room, so nothing moves.
 */
export function AgentSession({ progress, className = '' }: { progress: number; className?: string }) {
  return (
    <MacWindow title="~/code/breeze" tone="dark" className={className}>
      <div className="flex h-full flex-col px-4 pt-3.5 pb-3 font-mono text-[11.5px] leading-[17px]">
        <div
          className="flex gap-2 rounded-md px-2.5 py-2"
          style={{ background: 'rgba(255,255,255,0.06)', color: text }}
        >
          <span style={{ color: dim }}>&gt;</span>
          <span className="min-w-0">{showcase.prompt}</span>
        </div>
        <div className="flex flex-col">
          {lines.map((line, index) => {
            const opacity = shown(progress, line.at);
            return (
              <div
                key={index}
                className={line.spaced ? 'mt-3' : 'mt-[3px]'}
                style={{ opacity, transform: `translate3d(0, ${(1 - opacity) * 4}px, 0)` }}
              >
                {line.body}
              </div>
            );
          })}
        </div>
        <div className="mt-auto pt-3">
          {progress > 0.02 && progress < 0.84 && (
            <div className="mb-2 flex gap-2" style={{ color: '#e5a25c' }}>
              <span>✻</span>
              <span>Working…</span>
            </div>
          )}
          <div
            className="flex items-center gap-2 rounded-md px-2.5 py-[7px]"
            style={{ border: '1px solid rgba(255,255,255,0.14)', color: dim }}
          >
            <span>&gt;</span>
            <span className="h-[14px] w-[7px]" style={{ background: 'rgba(255,255,255,0.7)' }} />
          </div>
        </div>
      </div>
    </MacWindow>
  );
}
