import {
  AppWindow,
  AudioLines,
  Bell,
  Blocks,
  Box,
  Camera,
  CirclePlay,
  Clipboard,
  Cpu,
  FolderOpen,
  Globe,
  Keyboard,
  Layers,
  type LucideIcon,
  PanelTop,
  Settings2,
  SquareMenu,
  SquareTerminal,
  Type,
  Undo2,
} from 'lucide-react';

/** The sign of each page that has one, by the last part of its address. */
const icons: Record<string, LucideIcon> = {
  windows: AppWindow,
  back: Undo2,
  'system-bars': PanelTop,
  keys: Keyboard,
  system: Settings2,
  webgpu: Box,
  compositing: Layers,
  fonts: Type,
  'media-players': CirclePlay,
  cameras: Camera,
  audio: AudioLines,
  menus: SquareMenu,
  'file-dialogs': FolderOpen,
  notifications: Bell,
  clipboard: Clipboard,
  processes: SquareTerminal,
  network: Globe,
  workers: Cpu,
  waft: Blocks,
};

/** The sign of the page at `url`, a plain box where it has none of its own. */
export function topicIcon(url: string): LucideIcon {
  return icons[url.split('/').pop() ?? ''] ?? Box;
}
