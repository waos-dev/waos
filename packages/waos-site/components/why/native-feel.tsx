import { ChevronLeft, Search, SquarePen } from 'lucide-react';
import { AndroidPhone } from '../showcase/android-phone';
import { fluent } from '../showcase/fluent';
import { MacWindow } from '../showcase/mac-window';

/**
 * One app's native feel, close up: on a Mac, Notes in Fluent's controls with macOS's own menu bar
 * open at File, its items and shortcuts as `builtin-apps/notes/src/menus.rs` sets them; on an
 * Android phone, Inspector's network panel in Cupertino's, as its phone layout draws it
 * (`builtin-apps/inspector/src/ui/cupertino/panel.rs`), a finger on its segmented control.
 */
export function NativeFeel() {
  return (
    <div className="relative h-[524px] overflow-hidden bg-linear-to-br from-[#dfe9fb] via-[#ece6f8] to-[#f8e9e4] sm:h-[500px] dark:from-[#16203a] dark:via-[#21183a] dark:to-[#341f23]">
      <MenuBar />
      <NotesWindow />
      <FileMenu />
      <div className="absolute top-[316px] right-4 sm:top-[200px] sm:right-6">
        <AndroidPhone width={262} time="10:44">
          <InspectorNetwork />
        </AndroidPhone>
      </div>
    </div>
  );
}

const menuTitles = ['File', 'Edit', 'View', 'Window'];

/** macOS's menu bar, as Notes sets it: its own menu, then File, Edit, View and Window. */
function MenuBar() {
  return (
    <div className="absolute inset-x-0 top-0 flex h-7 items-center gap-0.5 bg-white/55 px-3 text-[13px] text-black/85 backdrop-blur-xl dark:bg-black/35 dark:text-white/90">
      <span className="px-2 font-semibold">Notes</span>
      {menuTitles.map((title) => (
        <span
          key={title}
          className={`rounded-[5px] px-2 py-[1px] ${title === 'File' ? 'bg-black/10 dark:bg-white/20' : ''} ${title === 'Window' ? 'hidden sm:inline' : ''}`}
        >
          {title}
        </span>
      ))}
      <span className="ml-auto hidden px-2 tabular-nums sm:inline">Thu 10:44</span>
    </div>
  );
}

type MenuEntry = { label: string; keys?: string; disabled?: boolean } | 'separator';

/** Notes' File menu, in its groups; Close File and Recover are greyed while no file is open and no note is deleted. */
const fileMenu: readonly MenuEntry[] = [
  { label: 'New Note', keys: '⌘N' },
  'separator',
  { label: 'Open…', keys: '⌘O' },
  { label: 'Close File', disabled: true },
  'separator',
  { label: 'Import…' },
  { label: 'Export…' },
  'separator',
  { label: 'Delete', keys: '⌘⌫' },
  { label: 'Recover', disabled: true },
  'separator',
  { label: 'Remind Me in 1 Minute' },
  'separator',
  { label: 'Close', keys: '⌘W' },
];

function FileMenu() {
  return (
    <div
      className="absolute top-[30px] left-[58px] w-[232px] rounded-[10px] bg-[#f6f6f6]/90 p-[5px] text-[13px] text-black/85 backdrop-blur-2xl sm:left-[72px] dark:bg-[#2b2b2d]/90 dark:text-white/90"
      style={{
        boxShadow:
          '0 0 0 0.5px rgba(0,0,0,0.22), inset 0 0 0 0.5px rgba(255,255,255,0.35), 0 16px 40px -8px rgba(15,23,42,0.35), 0 4px 10px -4px rgba(15,23,42,0.2)',
      }}
    >
      {fileMenu.map((entry, index) =>
        entry === 'separator' ? (
          <div key={index} className="mx-2.5 my-[5px] h-px bg-black/10 dark:bg-white/12" />
        ) : (
          <div
            key={entry.label}
            className={`flex h-[22px] items-center justify-between rounded-[5px] px-2.5 ${
              index === 0 ? 'bg-[#0a63e0] text-white' : ''
            } ${entry.disabled ? 'text-black/30 dark:text-white/30' : ''}`}
          >
            <span>{entry.label}</span>
            {entry.keys && (
              <span className={`tracking-[0.06em] ${index === 0 ? 'text-white/90' : 'text-black/45 dark:text-white/45'}`}>
                {entry.keys}
              </span>
            )}
          </div>
        ),
      )}
    </div>
  );
}

const notes = [
  { title: 'Trip to Lisbon', said: 'Pastéis de Belém before ten, then the tram up to Alfama.', at: '10:41' },
  { title: 'Groceries', said: 'Oranges, coffee beans, oat milk, basil.', at: 'Yesterday' },
  { title: 'Breeze ideas', said: 'Hourly rain chance under each hour.', at: 'Monday' },
  { title: 'Reading list', said: 'The Rust Book, chapter 16.', at: 'Monday' },
];

/** Notes in Fluent's look: the list of notes beside the one open. */
function NotesWindow() {
  return (
    <MacWindow
      title="Notes"
      className="absolute top-[56px] left-[52px] hidden h-[388px] w-[540px] sm:flex"
    >
      <div className="flex size-full" style={{ background: fluent.layer, color: fluent.text }}>
        <div className="flex w-[262px] shrink-0 flex-col" style={{ background: fluent.base, borderRight: `1px solid ${fluent.stroke}` }}>
          <div className="flex items-center justify-between px-3 pt-2.5 pb-2">
            <span className="text-[13px] font-semibold">Notes</span>
            <SquarePen aria-hidden className="size-3.5" style={{ color: fluent.secondary }} strokeWidth={1.75} />
          </div>
          <div
            className="mx-2.5 flex h-6 items-center gap-1.5 rounded-[4px] px-2 text-[11px]"
            style={{ background: fluent.card, boxShadow: `0 0 0 1px ${fluent.stroke}, 0 1px 0 rgba(0,0,0,0.3)`, color: fluent.tertiary }}
          >
            <Search aria-hidden className="size-3" />
            Search
          </div>
          <div className="mt-2 flex flex-col gap-0.5 px-1.5">
            {notes.map((note, index) => (
              <div
                key={note.title}
                className="relative rounded-[4px] px-2.5 py-1.5"
                style={{ background: index === 0 ? 'rgba(0,0,0,0.055)' : undefined }}
              >
                {index === 0 && (
                  <span className="absolute top-1/2 left-0 h-4 w-[3px] -translate-y-1/2 rounded-full" style={{ background: fluent.accent }} />
                )}
                <p className="truncate text-[11.5px] font-semibold">{note.title}</p>
                <p className="truncate text-[10.5px]" style={{ color: fluent.secondary }}>
                  <span style={{ color: fluent.text }}>{note.at}</span> {note.said}
                </p>
              </div>
            ))}
          </div>
        </div>
        <div className="min-w-0 flex-1 px-5 pt-4">
          <p className="text-[10.5px]" style={{ color: fluent.tertiary }}>
            Thursday at 10:41
          </p>
          <p className="mt-2 text-[17px] font-semibold tracking-tight">Trip to Lisbon</p>
          <div className="mt-2 space-y-2 text-[11.5px] leading-relaxed" style={{ color: fluent.text }}>
            <p>Pastéis de Belém before ten, then the tram up to Alfama.</p>
            <p>Miradouro da Senhora do Monte at sunset.</p>
            <p>
              Dinner near Cais do Sodré<span className="ml-px inline-block h-3.5 w-px translate-y-0.5 bg-black/80" />
            </p>
          </div>
        </div>
      </div>
    </MacWindow>
  );
}

const cupertino = {
  blue: '#007aff',
  label: '#000000',
  secondary: 'rgba(60,60,67,0.6)',
  separator: 'rgba(60,60,67,0.18)',
  grouped: '#f2f2f7',
  fill: 'rgba(118,118,128,0.12)',
  red: '#ff3b30',
} as const;

const tiles: readonly { title: string; details: string[]; beneath?: string }[] = [
  { title: 'GET https://api.open-meteo.com/v1/forecast', details: ['10:42:07', '200', '184 ms', 'Allowed'] },
  {
    title: 'POST https://telemetry.skyfeed.dev/v1/events',
    details: ['10:42:07', 'Refused'],
    beneath: "The domain is not among those it may reach. Change it under Network, in its settings in WaOS's library.",
  },
  { title: 'GET https://api.open-meteo.com/v1/forecast', details: ['10:42:09', '200', '171 ms', 'Allowed'] },
];

/** Inspector's network panel at a phone's width: its navigation bar, filter, segments and rows. */
function InspectorNetwork() {
  return (
    <div className="relative flex size-full flex-col" style={{ background: cupertino.grouped, color: cupertino.label }}>
      <div className="grid h-10 shrink-0 grid-cols-[1fr_auto_1fr] items-center gap-1 pr-3 pl-1 text-[11.5px]" style={{ borderBottom: `0.5px solid ${cupertino.separator}`, background: 'rgba(249,249,249,0.94)' }}>
        <span className="flex items-center" style={{ color: cupertino.blue }}>
          <ChevronLeft aria-hidden className="size-[18px]" strokeWidth={2.4} />
          Report
        </span>
        <span className="text-[13px] font-semibold">Network</span>
        <span className="flex justify-end gap-2" style={{ color: cupertino.blue }}>
          <span>Stop</span>
          <span>Restart</span>
        </span>
      </div>
      <div className="flex flex-col gap-2 px-3 pt-2.5">
        <div className="flex h-7 items-center gap-1.5 rounded-[9px] px-2 text-[12.5px]" style={{ background: cupertino.fill, color: cupertino.secondary }}>
          <Search aria-hidden className="size-3.5" strokeWidth={2.2} />
          Filter
        </div>
        <div className="relative grid h-[26px] grid-cols-2 overflow-hidden rounded-[5px] text-[12px] font-medium" style={{ boxShadow: `inset 0 0 0 1px ${cupertino.blue}` }}>
          <span className="flex items-center justify-center text-white" style={{ background: cupertino.blue }}>
            All
          </span>
          <span className="flex items-center justify-center" style={{ color: cupertino.blue, borderLeft: `1px solid ${cupertino.blue}`, background: 'rgba(0,122,255,0.14)' }}>
            Refused
          </span>
        </div>
      </div>
      <p className="px-6 pt-4 pb-1.5 text-[11px]" style={{ color: cupertino.secondary }}>
        Breeze
      </p>
      <div className="mx-3 overflow-hidden rounded-[10px] bg-white">
        {tiles.map((tile, index) => (
          <div key={index} className="py-2 pr-3 pl-3" style={{ borderTop: index ? `0.5px solid ${cupertino.separator}` : undefined }}>
            <div className="min-w-0">
              <p className="truncate font-mono text-[10.5px]">{tile.title}</p>
              <p className="mt-0.5 flex items-center gap-1 text-[10.5px] whitespace-nowrap" style={{ color: cupertino.secondary }}>
                {tile.details.map((said, at) => (
                  <span key={said} className="flex items-center gap-1" style={{ color: said === 'Refused' ? cupertino.red : undefined }}>
                    {at > 0 && <span className="size-[2.5px] rounded-full" style={{ background: cupertino.secondary }} />}
                    {said}
                  </span>
                ))}
              </p>
              {tile.beneath && (
                <p className="mt-0.5 line-clamp-2 text-[10.5px] leading-snug" style={{ color: cupertino.secondary }}>
                  {tile.beneath}
                </p>
              )}
            </div>
          </div>
        ))}
      </div>
      <span
        className="pointer-events-none absolute top-[81px] right-[49px] size-9 rounded-full"
        style={{ background: 'rgba(120,120,128,0.28)', boxShadow: '0 0 0 1px rgba(255,255,255,0.6), 0 2px 8px rgba(0,0,0,0.12)' }}
      />
    </div>
  );
}
