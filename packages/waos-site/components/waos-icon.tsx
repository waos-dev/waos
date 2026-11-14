import icon from '@/app/icon.svg';

/**
 * WaOS's icon: three waves in the page's colours, one behind another, rising in a white app tile.
 * The drawing is `app/icon.svg`, the browser tab's too, shown as an image: drawn in the page
 * itself, two of them would share the names of its colours, and one in a hidden part of the page
 * leaves the other blank.
 */
export function WaosIcon({ size, className }: { size: number; className?: string }) {
  // eslint-disable-next-line @next/next/no-img-element -- a vector, which nothing would resize
  return <img src={icon.src} width={size} height={size} alt="WaOS" className={className} />;
}
