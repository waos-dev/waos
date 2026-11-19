import type { ReactNode } from 'react';
import { ArrowLeft } from 'lucide-react';
import { fluent } from '../showcase/fluent';
import { MacWindow } from '../showcase/mac-window';

/** The program the moment follows, a document scanner. */
const program = 'Scanner';

/**
 * A program's settings in WaOS's library and the question it asks, as WaOS's Fluent layouts draw
 * them, in their words (`apps/waos/src/ui/library/settings/fluent.rs`, `library/words.rs` and
 * `ui/question_sheet.rs`): Scanner reaches only the domain listed and the folder added; its
 * camera follows the setting for every program, Ask, so its first use asks, over its window; its
 * microphone is answered Simulated, WaOS's own.
 */
export function PermissionMoment() {
  return (
    <div className="relative flex flex-col px-4 pt-5 pb-5 sm:px-8 sm:pt-8 sm:pb-8">
      <MacWindow title="WaOS" className="w-full">
        <Settings />
      </MacWindow>
      <MacWindow title={program} className="relative -mt-[52px] ml-auto w-[88%] sm:-mt-[262px] sm:-mr-3 sm:w-[248px]">
        <Asked />
      </MacWindow>
    </div>
  );
}

function Settings() {
  return (
    <div className="relative overflow-hidden px-4 pt-3 pb-16 sm:h-[560px] sm:pb-0" style={{ background: fluent.base, color: fluent.text }}>
      <div className="flex items-center gap-1">
        <span className="flex size-7 items-center justify-center rounded-[4px]">
          <ArrowLeft aria-hidden className="size-4" strokeWidth={1.6} />
        </span>
        <span className="text-[16px] font-semibold tracking-tight">{program}&apos;s settings</span>
      </div>

      <Name>Network</Name>
      <FollowButton />
      <Toggle />
      <Radios choices={['Any domain', 'Only these domains', 'All domains but these']} checked="Only these domains" />
      <Listed>api.scanner.app</Listed>

      <Name>Folders</Name>
      <FollowButton />
      <Caption>A folder added or removed applies at once, to a program running too.</Caption>
      <Listed>/Users/you/Documents/Scans, writable</Listed>

      <Name>Camera</Name>
      <Caption>Follows the setting for every program.</Caption>
      <Radios choices={['Allow', "Don't allow", 'Simulated']} />

      <Name>Microphone</Name>
      <FollowButton />
      <Radios choices={['Allow', "Don't allow", 'Simulated']} checked="Simulated" />
      <Caption narrow>
        Simulated: a microphone of WaOS&apos;s own, not yours. Nothing asks the system, and the program is shown using a
        simulated microphone.
      </Caption>
      <div className="hidden sm:block">
        <Name>What it records</Name>
        <Radios choices={['A quiet beep', 'Silence']} checked="A quiet beep" />
      </div>
      <span className="pointer-events-none absolute inset-x-0 bottom-0 h-16" style={{ background: `linear-gradient(transparent, ${fluent.base})` }} />
    </div>
  );
}

/** A setting's name, over its controls, as `setting_name` sets it. */
function Name({ children }: { children: ReactNode }) {
  return <p className="mt-4 mb-1 text-[12.5px] font-semibold">{children}</p>;
}

/** Words under a setting; `narrow` keeps them clear of the question drawn over the page's corner. */
function Caption({ children, narrow }: { children: ReactNode; narrow?: boolean }) {
  return (
    <p className={`mt-0.5 mb-1 text-[11px] leading-snug ${narrow ? 'sm:max-w-[250px]' : ''}`} style={{ color: fluent.secondary }}>
      {children}
    </p>
  );
}

const control = {
  background: fluent.card,
  boxShadow: `0 0 0 1px ${fluent.stroke}, 0 1px 0 ${fluent.strokeStrong}`,
};

function Button({ children }: { children: ReactNode }) {
  return (
    <span className="inline-flex h-6 shrink-0 items-center rounded-[4px] px-2.5 text-[11px]" style={control}>
      {children}
    </span>
  );
}

/** What a setting set for this program alone shows: the way back to the setting for every program. */
function FollowButton() {
  return (
    <div className="mb-1">
      <Button>Use the setting for every program</Button>
    </div>
  );
}

function Toggle() {
  return (
    <div className="my-1 flex items-center gap-2 text-[11.5px]">
      <span className="relative h-[18px] w-9 rounded-full" style={{ background: fluent.accent }}>
        <span className="absolute top-[4px] right-[4px] size-[10px] rounded-full bg-white" />
      </span>
      On
    </div>
  );
}

function Radios({ choices, checked }: { choices: string[]; checked?: string }) {
  return (
    <div className="my-1 flex flex-wrap gap-x-3.5 gap-y-1.5 text-[11.5px]">
      {choices.map((choice) => (
        <span key={choice} className="flex items-center gap-1.5">
          {choice === checked ? (
            <span className="flex size-4 items-center justify-center rounded-full" style={{ background: fluent.accent }}>
              <span className="size-[7px] rounded-full bg-white" />
            </span>
          ) : (
            <span className="size-4 rounded-full bg-black/[0.02]" style={{ boxShadow: 'inset 0 0 0 1px rgba(0,0,0,0.45)' }} />
          )}
          {choice}
        </span>
      ))}
    </div>
  );
}

/** One of a rule's list, with Remove. */
function Listed({ children }: { children: ReactNode }) {
  return (
    <div className="mt-1.5 flex items-center gap-2 text-[11.5px]">
      <span className="min-w-0 flex-1 break-words">{children}</span>
      <Button>Remove</Button>
    </div>
  );
}

/** The question over the program's window: Fluent's sheet, with Don't allow and Allow. */
function Asked() {
  return (
    <div className="flex h-[268px] flex-col bg-[#1c1c20]">
      <div className="relative grid min-h-0 flex-1 place-items-center">
        <div className="relative h-[64%] w-[38%] rounded-[3px] bg-white/[0.07]">
          <span className="absolute -top-1 -left-1 size-3 rounded-tl-[3px] border-t-2 border-l-2 border-white/60" />
          <span className="absolute -top-1 -right-1 size-3 rounded-tr-[3px] border-t-2 border-r-2 border-white/60" />
          <span className="absolute -bottom-1 -left-1 size-3 rounded-bl-[3px] border-b-2 border-l-2 border-white/60" />
          <span className="absolute -right-1 -bottom-1 size-3 rounded-br-[3px] border-r-2 border-b-2 border-white/60" />
        </div>
        <span className="absolute inset-0 bg-black/35" />
      </div>
      <div className="flex flex-col gap-3 p-4" style={{ background: fluent.base, color: fluent.text }}>
        <p className="text-[14.5px] leading-snug font-semibold tracking-tight">Allow {program} to use the camera?</p>
        <p className="text-[12px] leading-snug" style={{ color: fluent.secondary }}>
          You can change this in its settings in WaOS&apos;s library.
        </p>
        <div className="flex justify-end gap-2 pt-0.5">
          <span className="inline-flex h-7 items-center rounded-[4px] px-3 text-[12px]" style={control}>
            Don&apos;t allow
          </span>
          <span
            className="inline-flex h-7 items-center rounded-[4px] px-4 text-[12px] text-white"
            style={{ background: fluent.accent, boxShadow: '0 1px 0 rgba(0,0,0,0.25)' }}
          >
            Allow
          </span>
        </div>
      </div>
    </div>
  );
}
