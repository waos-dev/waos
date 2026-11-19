import type { ReactNode } from 'react';

/** The three apps both sides of the comparison run: WaOS's own, by their icons in `public/apps`. */
const apps = [
  { name: 'Notes', icon: '/apps/notes.png' },
  { name: 'Reader', icon: '/apps/reader.png' },
  { name: 'Synth', icon: '/apps/synth.png' },
] as const;

/**
 * The same three apps two ways: wrapped in a browser, each carrying its own browser engine and
 * JavaScript runtime, beside WaOS, where each is a small WebAssembly module over one runtime
 * they share. Both stacks stand on one baseline, so the difference in weight is what the eye sees
 * first; there are no numbers in it, as it measures nothing.
 */
export function RuntimeDiagram() {
  return (
    <div className="grid h-full divide-y divide-fd-border sm:grid-cols-2 sm:divide-x sm:divide-y-0">
      <Side title="Web-wrapped apps" detail="A browser engine in every app">
        <div className="grid grid-cols-3 gap-2.5">
          {apps.map((app) => (
            <Tower key={app.name} app={app}>
              <Block className="h-9 border-fd-border bg-fd-background">App code</Block>
              <Block className="h-14 border-amber-600/20 bg-amber-500/[0.09] text-amber-900/75 dark:border-amber-300/15 dark:text-amber-100/70">
                JavaScript runtime
              </Block>
              <Block className="h-36 border-slate-500/25 bg-slate-500/[0.13] text-slate-700 [background-image:repeating-linear-gradient(135deg,transparent_0_7px,rgba(100,116,139,0.13)_7px_8px)] dark:border-slate-300/15 dark:bg-slate-300/[0.08] dark:text-slate-300">
                Browser engine
              </Block>
            </Tower>
          ))}
        </div>
        <Brace>One engine per app</Brace>
      </Side>
      <Side title="WaOS apps" detail="One runtime they all share">
        <div className="grid grid-cols-3 gap-2.5">
          {apps.map((app) => (
            <Tower key={app.name} app={app}>
              <Block className="h-9 border-[#654ff0]/25 bg-[#654ff0]/[0.08] text-[#4936d6] dark:border-[#a99dff]/25 dark:text-[#c2b9ff]">
                Wasm
              </Block>
            </Tower>
          ))}
        </div>
        <div className="relative mt-1.5 flex h-14 flex-col items-center justify-center overflow-hidden rounded-xl text-white shadow-[0_10px_30px_-12px_rgba(168,85,247,0.55)]">
          <span className="absolute inset-0 bg-linear-to-r from-[#3b82f6] via-[#d946ef] to-[#fb923c]" />
          <span className="absolute inset-0 bg-linear-to-b from-white/20 to-transparent" />
          <span className="relative text-[13px] font-semibold tracking-tight">WaOS runtime</span>
          <span className="relative text-[11px] text-white/85">GPU-accelerated, shared</span>
        </div>
        <Brace>One for every app</Brace>
      </Side>
    </div>
  );
}

function Side({ title, detail, children }: { title: string; detail: string; children: ReactNode }) {
  return (
    <div className="flex flex-col px-5 pt-6 pb-5 sm:min-h-[400px] sm:px-6 lg:px-7 lg:pt-8">
      <p className="text-[15px] font-semibold tracking-tight">{title}</p>
      <p className="mt-0.5 text-[13px] text-fd-muted-foreground">{detail}</p>
      <div className="mt-6 flex flex-1 flex-col justify-end">{children}</div>
    </div>
  );
}

function Tower({ app, children }: { app: { name: string; icon: string }; children: ReactNode }) {
  return (
    <div className="flex flex-col gap-1.5">
      <div className="mb-1 flex flex-col items-center gap-1">
        {/* eslint-disable-next-line @next/next/no-img-element -- a small icon the site already serves */}
        <img src={app.icon} alt="" width={36} height={36} className="size-9 rounded-[9px] shadow-sm" />
        <span className="text-[11px] font-medium text-fd-muted-foreground">{app.name}</span>
      </div>
      {children}
    </div>
  );
}

function Block({ className, children }: { className: string; children: ReactNode }) {
  return (
    <div
      className={`flex items-center justify-center rounded-lg border px-1.5 text-center text-[11px] leading-tight font-medium ${className}`}
    >
      {children}
    </div>
  );
}

/** A bracket under the stacks it speaks of, and what it says of them. */
function Brace({ children }: { children: ReactNode }) {
  return (
    <div className="mt-3 flex flex-col items-center">
      <span className="h-1.5 w-full rounded-b-md border-x border-b border-fd-foreground/20" />
      <span className="mt-1.5 text-[11.5px] text-fd-muted-foreground">{children}</span>
    </div>
  );
}
