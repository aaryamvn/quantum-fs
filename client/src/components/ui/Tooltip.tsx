import { cloneElement, useCallback, useEffect, useRef, useState } from "react";
import type { ReactElement, Ref } from "react";

import { Popover } from "./Popover";
import type { Placement } from "./Popover";

export interface TooltipProps {
  label: string;
  shortcut?: string;
  children: ReactElement;
  placement?: Placement;
  delay?: number;
}

type AnyProps = Record<string, unknown>;

function call(fn: unknown, e: unknown): void {
  if (typeof fn === "function") (fn as (ev: unknown) => void)(e);
}

/**
 * The label a control earns only if you hesitate. The delay is the whole point:
 * tooltips that fire instantly turn a cursor crossing the toolbar into a strobe,
 * so nothing appears until the pointer has settled, and nothing appears at all
 * while a button is being pressed — by then you have already committed and a
 * label explaining what you just did is noise.
 *
 * Registered as a non-dismissable layer, so Escape still belongs to whatever
 * dialog or menu is underneath.
 */
export function Tooltip({
  label,
  shortcut,
  children,
  placement = "top",
  delay = 450,
}: TooltipProps) {
  const [anchor, setAnchor] = useState<HTMLElement | null>(null);
  const [open, setOpen] = useState(false);
  const timer = useRef<number | null>(null);
  const down = useRef(false);

  const cancel = useCallback(() => {
    if (timer.current !== null) {
      window.clearTimeout(timer.current);
      timer.current = null;
    }
  }, []);

  const hide = useCallback(() => {
    cancel();
    setOpen(false);
  }, [cancel]);

  const show = useCallback(
    (ms: number) => {
      cancel();
      if (down.current) return;
      timer.current = window.setTimeout(() => {
        timer.current = null;
        setOpen(true);
      }, ms);
    },
    [cancel],
  );

  // Pressed state is tracked globally: the press may start on the child and end
  // anywhere, and a drag that began elsewhere must not raise a tooltip either.
  useEffect(() => {
    const onDown = () => {
      down.current = true;
      cancel();
      setOpen(false);
    };
    const onUp = () => {
      down.current = false;
    };
    window.addEventListener("pointerdown", onDown, true);
    window.addEventListener("pointerup", onUp, true);
    window.addEventListener("pointercancel", onUp, true);
    return () => {
      window.removeEventListener("pointerdown", onDown, true);
      window.removeEventListener("pointerup", onUp, true);
      window.removeEventListener("pointercancel", onUp, true);
      cancel();
    };
  }, [cancel]);

  const props = children.props as AnyProps;
  const childRef = props.ref as Ref<HTMLElement> | undefined;

  const setRef = useCallback(
    (el: HTMLElement | null) => {
      setAnchor(el);
      if (typeof childRef === "function") childRef(el);
      else if (childRef && typeof childRef === "object") {
        (childRef as { current: HTMLElement | null }).current = el;
      }
    },
    [childRef],
  );

  const clone = cloneElement(children as ReactElement<AnyProps>, {
    ref: setRef,
    onPointerEnter: (e: unknown) => {
      call(props.onPointerEnter, e);
      show(delay);
    },
    onPointerLeave: (e: unknown) => {
      call(props.onPointerLeave, e);
      hide();
    },
    // Keyboard focus has already paid the hesitation cost, so it shows at once.
    onFocus: (e: unknown) => {
      call(props.onFocus, e);
      show(0);
    },
    onBlur: (e: unknown) => {
      call(props.onBlur, e);
      hide();
    },
  } as AnyProps);

  return (
    <>
      {clone}
      <Popover
        open={open && anchor !== null}
        onClose={hide}
        anchor={anchor}
        placement={placement}
        offset={8}
        kind="tooltip"
        className="pointer-events-none px-[8px] py-[5px] text-[11.5px] leading-none text-fg"
      >
        {label}
        {shortcut ? <span className="ml-[6px] text-fg-3">{shortcut}</span> : null}
      </Popover>
    </>
  );
}

export default Tooltip;
