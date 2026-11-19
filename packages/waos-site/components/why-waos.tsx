import Link from 'next/link';
import type { ReactNode } from 'react';
import { docsRoute } from '@/lib/shared';
import { guideTopics } from '@/lib/source';
import { CapabilityMoments } from './why/capability-moments';
import { NativeFeel } from './why/native-feel';
import { PermissionMoment } from './why/permission-moment';
import { RuntimeDiagram } from './why/runtime-diagram';

interface Feature {
  id: string;
  eyebrow: string;
  title: string;
  text: string;
  more?: ReactNode;
  picture: ReactNode;
}

function features(): readonly Feature[] {
  return [
    {
      id: 'light',
      eyebrow: 'Rust and WebAssembly',
      title: 'Fast, and light on memory and CPU.',
      text: 'WaOS apps are written in Rust and compiled to WebAssembly. Every app you open shares one GPU-accelerated WaOS runtime, instead of bringing a browser engine and a JavaScript runtime of its own. Apps start fast, use little memory and leave the CPU quiet.',
      picture: <RuntimeDiagram />,
    },
    {
      id: 'native',
      eyebrow: 'One .wapk, native everywhere',
      title: 'Write once. It feels like a real app.',
      text: "The same .wapk runs on macOS and Android, with more platforms coming. On the desktop it gets real windows, the system's menu bar and keyboard shortcuts; on a phone, touch that feels at home, through Waft's Fluent and Cupertino kits. Not a web page in a frame.",
      picture: <NativeFeel />,
    },
    {
      id: 'capable',
      eyebrow: 'Typed system APIs',
      title: 'Rich APIs, enough for real software.',
      text: 'From a quick daily utility to a GPU-accelerated video editor. Typed APIs for graphics, media, files, networking and multitasking are ready out of the box.',
      more: (
        <Link
          href={`${docsRoute}/guide`}
          className="mt-6 inline-flex items-center gap-1 font-medium text-fd-foreground underline-offset-4 hover:underline"
        >
          Browse all {guideTopics().length} guides <span aria-hidden>→</span>
        </Link>
      ),
      picture: <CapabilityMoments />,
    },
    {
      id: 'yours',
      eyebrow: 'Permissions',
      title: 'Powerful, and always under your control.',
      text: "Power doesn't have to mean blind trust. Each app reaches only the folders and domains you allow, and asks before it first uses the camera, the microphone or notifications. You can even hand it a simulated camera or microphone, and watch every use live in Inspector.",
      picture: <PermissionMoment />,
    },
  ];
}

/**
 * Why an app made for WaOS is worth making, in four rows that alternate their picture's side on
 * wide screens: light on the machine, native on each platform, capable, and under the user's
 * control. Each picture shows a fact of WaOS, drawn from its apps and their words; none invents a
 * number. Narrower, each row is its words over its picture.
 */
export function WhyWaos() {
  return (
    <section id="why-waos" aria-labelledby="why-waos-title" className="border-t">
      <div className="mx-auto w-full max-w-[1120px] px-6 pt-28 pb-12 lg:pb-16">
        <div data-head className="max-w-[760px]">
          <p className="text-sm font-medium text-fd-muted-foreground">Why WaOS apps</p>
          <h2 id="why-waos-title" className="mt-2 text-4xl leading-[1.08] font-semibold tracking-tight sm:text-5xl">
            Light, native and capable.
            <br />
            <span className="text-fd-muted-foreground">And always yours to control.</span>
          </h2>
        </div>
        <div className="mt-14 lg:mt-20">
          {features().map((feature, index) => (
            <FeatureRow key={feature.id} feature={feature} flipped={index % 2 === 1} first={index === 0} />
          ))}
        </div>
      </div>
    </section>
  );
}

function FeatureRow({ feature, flipped, first }: { feature: Feature; flipped: boolean; first: boolean }) {
  return (
    <div
      data-row
      id={`why-${feature.id}`}
      className={`grid items-center gap-10 py-14 lg:grid-cols-12 lg:gap-16 lg:py-20 ${first ? '' : 'border-t'}`}
    >
      <div className={`lg:col-span-5 ${flipped ? 'lg:order-last' : ''}`}>
        <p className="text-sm font-medium text-fd-muted-foreground">{feature.eyebrow}</p>
        <h3 className="mt-3 text-3xl leading-[1.12] font-semibold tracking-tight text-balance sm:text-[40px]">
          {feature.title}
        </h3>
        <p className="mt-5 max-w-[520px] text-lg leading-relaxed text-pretty text-fd-muted-foreground">{feature.text}</p>
        {feature.more}
      </div>
      <div aria-hidden className="overflow-hidden rounded-3xl border bg-fd-card lg:col-span-7">
        {feature.picture}
      </div>
    </div>
  );
}
