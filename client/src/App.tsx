import {
  animate,
  AnimatePresence,
  LayoutGroup,
  motion,
  useMotionValue,
  useReducedMotion,
} from "motion/react";
import type { AnimationPlaybackControls } from "motion/react";
import { useCallback, useEffect, useRef, useState } from "react";

import { useAppShell } from "@/app/appShell";
import { DragRegion } from "@/components/chrome/DragRegion";
import { IconGallery } from "@/features/dev/IconGallery";
import { HomeScreen } from "@/features/home";
import { OnboardingScreen } from "@/features/onboarding";
import { ColorField } from "@/features/splash/ColorField";
import { GrainOverlay } from "@/features/splash/GrainOverlay";
import { resolveField } from "@/features/splash/shaders";
import { useSplashTimeline } from "@/features/splash/timeline";
import { WorkspaceScreen } from "@/features/workspace";
import { EASE } from "@/features/workspace/layout";
import { useWorkspace } from "@/features/workspace/store";
import { VaultDive } from "@/features/workspace/transition/VaultDive";
import { BackendProvider, useBackend } from "@/lib/backend";
import type { NodeId, Vault, VaultId } from "@/lib/backend";
import { devQuery } from "@/lib/devQuery";

/** Home recedes over this; coming back is shorter, because nothing is being revealed. */
const RECEDE_MS = 380;
const RETURN_MS = 260;
/** The workspace fades up under the dive's still-opaque fill. */
const WORKSPACE_IN_MS = 300;

/**
 * The onboarding → home handoff: one horizontal slide, the question leaving to
 * the left as the list arrives from the right. Shorter out than in, so the
 * answer feels taken rather than mulled over.
 */
const ONBOARD_OUT_MS = 320;
const HOME_SLIDE_IN_MS = 380;
const SLIDE_PX = 32;

/** The color field blooms while the dive expands, then drains back to its panel width. */
const PANEL_REST = 0.45;
const PANEL_BLOOM = 1.0;
const PANEL_UP_MS = 500;
const PANEL_DOWN_MS = 600;

/** Above the grain (z-1) so the home keeps the layering it has on its own. */
const HOME_Z = 2;

const HOME_AT_REST = { opacity: 1, scale: 1, filter: "blur(0px)", x: 0 } as const;
const HOME_RECEDED = { opacity: 0, scale: 0.97, filter: "blur(8px)", x: 0 } as const;
/** Only after onboarding: the list comes in from the right rather than out of a blur. */
const HOME_FROM_ONBOARDING = {
  opacity: 0,
  scale: 1,
  filter: "blur(0px)",
  x: SLIDE_PX,
} as const;

/** The vault open detail carried by the `qfs:open-vault` window event. */
interface OpenVaultDetail {
  vaultId?: VaultId;
  folderId?: NodeId | null;
  select?: NodeId[];
}

export default function App() {
  // Design QA surface: a contact sheet of every icon, with none of the app around it.
  if (devQuery.ui === "icons") return <IconGallery />;

  return (
    <BackendProvider>
      <Shell />
    </BackendProvider>
  );
}

/**
 * The app is one window with two worlds in it: the home list of vaults, and the
 * workspace inside one of them. This is the only place that knows which is on
 * screen, and it lives inside `BackendProvider` because entering a vault means
 * attaching the client to the workspace store first.
 *
 * The splash's color field and grain never unmount — they are two cheap fixed
 * layers, they are what the dive blooms, and keeping them means the trip back
 * to the overview is a crossfade over a background that was never torn down.
 */
function Shell() {
  const tl = useSplashTimeline();
  const shader = resolveField(devQuery.field);
  const reduced = useReducedMotion() ?? false;

  const { client } = useBackend();

  const screen = useAppShell((s) => s.screen);
  const pending = useAppShell((s) => s.pending);
  const enterVault = useAppShell((s) => s.enterVault);
  const startOnboarding = useAppShell((s) => s.startOnboarding);
  const finishOnboarding = useAppShell((s) => s.finishOnboarding);
  const switchVault = useAppShell((s) => s.switchVault);
  const diveDone = useAppShell((s) => s.diveDone);
  const goHome = useAppShell((s) => s.goHome);

  /** The store has a client and an identity: opening a vault can do something. */
  const [attached, setAttached] = useState(false);
  /**
   * The `seq` of the open whose workspace is already mounted. Matching against
   * the request rather than holding a boolean is what keeps a second dive from
   * showing the previous vault's workspace through its half-open aperture.
   */
  const [revealedSeq, setRevealedSeq] = useState(-1);

  /**
   * The field's panel width, as a motion value so the bloom costs no re-render.
   * It rests exactly where `ColorField` rests on its own.
   */
  const panel = useMotionValue(PANEL_REST);
  const panelAnims = useRef<AnimationPlaybackControls[]>([]);

  /** True once the shell has ever left home: only then is coming back a crossfade. */
  const leftHome = useRef(false);
  // Onboarding is *before* the home, not a place the home was left for: counting
  // it would make the list arrive out of the dive's blur instead of sliding in.
  if (screen.kind === "dive" || screen.kind === "workspace") leftHome.current = true;

  /** The home is arriving from the onboarding answer, so it slides rather than blurs. */
  const [fromOnboarding, setFromOnboarding] = useState(false);

  // One attach for the life of the app. Idempotent: it is a plain assignment in
  // the store, so a StrictMode double-mount costs nothing.
  useEffect(() => {
    let alive = true;
    void client
      .me()
      .then((me) => {
        if (!alive) return;
        useWorkspace.getState().attach(client, me);
        setAttached(true);
        // The counterpart of `qfs:me-failed`: the rail latches the failure at
        // module load, so a retry that works has to say so or every footer
        // mounted afterwards keeps reporting a disconnection that is over.
        window.dispatchEvent(new CustomEvent("qfs:me-ok"));
      })
      .catch(() => {
        // A missing identity is the backend's to report; the home still works.
        // The rail is told, so its footer stops saying "Connecting…" forever.
        if (alive) window.dispatchEvent(new CustomEvent("qfs:me-failed"));
      });
    return () => {
      alive = false;
    };
  }, [client]);

  // A machine nobody has named yet is asked once, before the list: the profile is
  // the daemon's (the mock keeps it in memory), so this is a read like any other.
  // A failure is silent on purpose — an unnamed profile is a nicety, and the home
  // list is still the app.
  useEffect(() => {
    let alive = true;
    void client
      .getProfile()
      .then((profile) => {
        if (!alive || profile.nameSet) return;
        startOnboarding();
      })
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, [client, startOnboarding]);

  // Screenshot path: `?vault=` lands straight in the workspace, no dive. The
  // splash is already skipped by devQuery, so there is nothing to wait for.
  useEffect(() => {
    if (devQuery.vault === null) return;
    switchVault(devQuery.vault, {
      folderId: devQuery.folder,
      select: devQuery.select?.split(","),
    });
  }, [switchVault]);

  // The sidebar, recents and search open vaults from deep inside the workspace;
  // they say so with a window event rather than reaching up through the tree.
  useEffect(() => {
    const onOpenVault = (event: Event) => {
      const detail = (event as CustomEvent<OpenVaultDetail>).detail;
      if (!detail?.vaultId) return;
      switchVault(detail.vaultId, { folderId: detail.folderId ?? null, select: detail.select });
    };
    const onHome = () => goHome();

    window.addEventListener("qfs:open-vault", onOpenVault);
    window.addEventListener("qfs:home", onHome);
    return () => {
      window.removeEventListener("qfs:open-vault", onOpenVault);
      window.removeEventListener("qfs:home", onHome);
    };
  }, [switchVault, goHome]);

  // Every open — dive, cross-vault switch or deep link — lands here. `pending`
  // is a fresh object per request, so re-opening the same vault still fires.
  useEffect(() => {
    if (!attached || pending === null) return;
    void useWorkspace
      .getState()
      .openVault(pending.vaultId, { folderId: pending.folderId, select: pending.select });
  }, [attached, pending]);

  // The field blooms with the aperture and drains as the fill dissolves, so the
  // color behind the whole window moves with the transition instead of sitting
  // still through it.
  useEffect(() => {
    if (reduced || screen.kind !== "dive") return;
    for (const a of panelAnims.current) a.stop();
    panelAnims.current = [animate(panel, PANEL_BLOOM, { duration: PANEL_UP_MS / 1000, ease: EASE })];
    const timer = setTimeout(() => {
      panelAnims.current.push(
        animate(panel, PANEL_REST, { duration: PANEL_DOWN_MS / 1000, ease: EASE }),
      );
    }, PANEL_UP_MS);
    return () => clearTimeout(timer);
  }, [reduced, screen, panel]);

  const handleOpenVault = useCallback(
    (vault: Vault, el: HTMLElement) => {
      const rect = el.getBoundingClientRect();
      enterVault(
        vault.id,
        vault.name,
        { x: rect.left, y: rect.top, w: rect.width, h: rect.height },
        {},
      );
    },
    [enterVault],
  );

  const handleOnboardingDone = useCallback(() => {
    setFromOnboarding(true);
    finishOnboarding();
    // The name just changed under `me()`, and the sidebar's profile row reads it.
    void client
      .me()
      .then((me) => {
        useWorkspace.getState().attach(client, me);
      })
      .catch(() => {});
  }, [client, finishOnboarding]);

  const handleReveal = useCallback(() => {
    const current = useAppShell.getState().pending;
    if (current !== null) setRevealedSeq(current.seq);
  }, []);

  const atHome = screen.kind === "home";
  const onboarding = screen.kind === "onboarding";
  const showWorkspace =
    screen.kind === "workspace" ||
    (screen.kind === "dive" && pending !== null && pending.seq === revealedSeq);

  return (
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
        panel={panel}
      />
      {/*
        Both of these belong to the overview, not to the vault.

        The grain is a 9% overlay blend: harmless over the splash's gradient,
        but over the workspace's surfaces it lifts every panel edge and every
        icon by a hair, and a file manager has to render flat.

        `DragRegion` is a fixed z-50 strip across the top of the window. Inside
        the workspace that strip lies over the top bar's carets and crumbs and
        would swallow their clicks, so the workspace uses its own header (and
        the sidebar's inset) as the drag handle instead.
      */}
      {screen.kind !== "workspace" ? (
        <>
          <GrainOverlay frozen={tl.frozen} />
          <DragRegion />
        </>
      ) : null}

      {/*
        One layout group over both halves of the transition: the dive's title and
        the workspace's root breadcrumb share `vault-title`, and the glide from
        one to the other only happens if Motion sees them as the same element.
      */}
      <LayoutGroup id="vault-shell">
        {/*
          The question lives where the home's column does, so the wordmark stays
          put while everything under it is exchanged: onboarding leaves to the
          left, the list arrives from the right. Gated on the splash exactly like
          the home, or the field would be typed into behind the intro.
        */}
        <AnimatePresence>
          {onboarding && tl.phase === "settled" ? (
            <motion.div
              key="onboarding"
              className="fixed inset-0"
              style={{ zIndex: HOME_Z }}
              initial={{ opacity: 1, x: 0 }}
              animate={{ opacity: 1, x: 0 }}
              exit={reduced ? { opacity: 0 } : { opacity: 0, x: -SLIDE_PX }}
              transition={reduced ? { duration: 0 } : { duration: ONBOARD_OUT_MS / 1000, ease: EASE }}
            >
              <OnboardingScreen onDone={handleOnboardingDone} />
            </motion.div>
          ) : null}
        </AnimatePresence>

        {screen.kind !== "workspace" && !onboarding ? (
          <motion.div
            className="fixed inset-0"
            // Sized to the window so the home's own `fixed` children keep their
            // geometry: a transformed ancestor is their containing block.
            style={{ zIndex: HOME_Z, pointerEvents: atHome ? "auto" : "none" }}
            initial={
              leftHome.current
                ? HOME_RECEDED
                : fromOnboarding && !reduced
                  ? HOME_FROM_ONBOARDING
                  : false
            }
            animate={atHome ? HOME_AT_REST : HOME_RECEDED}
            transition={
              reduced
                ? { duration: 0 }
                : {
                    duration:
                      (atHome
                        ? fromOnboarding && !leftHome.current
                          ? HOME_SLIDE_IN_MS
                          : RETURN_MS
                        : RECEDE_MS) / 1000,
                    ease: EASE,
                  }
            }
          >
            <HomeScreen revealed={tl.phase === "settled"} onOpenVault={handleOpenVault} />
          </motion.div>
        ) : null}

        {showWorkspace ? (
          <motion.div
            className="fixed inset-0"
            initial={screen.kind === "dive" && !reduced ? { opacity: 0 } : false}
            animate={{ opacity: 1 }}
            transition={reduced ? { duration: 0 } : { duration: WORKSPACE_IN_MS / 1000, ease: EASE }}
          >
            <WorkspaceScreen />
          </motion.div>
        ) : null}

        {screen.kind === "dive" ? (
          <VaultDive
            key={`${screen.vaultId}:${pending?.seq ?? 0}`}
            vaultId={screen.vaultId}
            vaultName={screen.vaultName}
            origin={screen.origin}
            onReveal={handleReveal}
            onDone={diveDone}
          />
        ) : null}
      </LayoutGroup>
    </div>
  );
}
