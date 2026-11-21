/**
 * The app the home page's story follows from prompt to desktop and phone. It is drawn for now;
 * give `screenshots` real captures of the app, each at its window's or screen's proportions, and
 * the story shows them in place of the drawing.
 */
export const showcase: Showcase = {
  name: 'Breeze',
  id: 'app.breeze',
  file: 'app.breeze-0.1.0.wapk',
  size: '2.1 MB',
  prompt: 'Read https://waos.app/agents.md and build a weather app called Breeze for desktop and phone.',
  screenshots: {},
};

export interface Showcase {
  name: string;
  /** Its id, as its manifest names it. */
  id: string;
  /** Its package, as `waos package` names it: `<id>-<version>.wapk`. */
  file: string;
  size: string;
  /** What the reader's agent is asked. */
  prompt: string;
  /** Captures of the running app; the drawn app shows where one is missing. */
  screenshots: { desktop?: string; phone?: string };
}
