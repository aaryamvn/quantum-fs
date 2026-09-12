import { DragRegion } from "@/components/chrome/DragRegion";
import { HomeScreen } from "@/features/home";
import { ColorField } from "@/features/splash/ColorField";
import { GrainOverlay } from "@/features/splash/GrainOverlay";
import { resolveField } from "@/features/splash/shaders";
import { useSplashTimeline } from "@/features/splash/timeline";
import { BackendProvider } from "@/lib/backend";
import { devQuery } from "@/lib/devQuery";

export default function App() {
  const tl = useSplashTimeline();
  const shader = resolveField(devQuery.field);

  return (
    <BackendProvider>
      <div
        className="relative h-full w-full bg-bg"
        onPointerDown={tl.phase !== "settled" ? tl.skip : undefined}
      >
        <ColorField
          progress={tl.progress}
          settle={tl.settle}
          time={tl.time}
          idle={tl.idle}
          shader={shader}
        />
        <GrainOverlay frozen={tl.frozen} />
        <DragRegion />
        <HomeScreen revealed={tl.phase === "settled"} />
      </div>
    </BackendProvider>
  );
}
