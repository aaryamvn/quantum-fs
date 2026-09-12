import { useId } from "react";

const ASPECT = 52 / 64;

type IconmarkProps = {
  /** Rendered width in px; height follows the 64:52 artwork ratio. */
  size?: number;
  className?: string;
};

/**
 * QuantumFS iconmark: three blurred lobes (coral, violet, black) forming the
 * mark. The Gaussian blur filter is part of the artwork, so it is kept inline
 * and given a per-instance id to avoid collisions between multiple instances.
 */
export function Iconmark({ size = 32, className }: IconmarkProps) {
  const filterId = `quantamfs-iconmark-blur-${useId()}`;

  return (
    <svg
      width={size}
      height={size * ASPECT}
      viewBox="0 0 64 52"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
      className={className}
      aria-hidden="true"
      focusable="false"
    >
      <g filter={`url(#${filterId})`}>
        <path
          d="M11.1 26.2585C11.1 34.3144 19.3783 40.845 29.5902 40.845C39.802 40.845 48.0804 34.3144 48.0804 26.2585C48.0804 18.2026 39.802 11.672 29.5902 11.672C19.3783 11.672 11.1 18.2026 11.1 26.2585Z"
          fill="#FF7B7B"
        />
        <path
          d="M38.3756 40.0412C47.6677 40.0412 47.2741 33.5625 47.2741 25.5706C47.2741 17.5787 47.6677 11.1 38.3756 11.1C29.0836 11.1 21.551 17.5787 21.551 25.5706C21.551 33.5625 29.0836 40.0412 38.3756 40.0412Z"
          fill="#4E0EFF"
        />
        <path
          d="M47.2765 37.6293C37.0646 37.6294 34.8157 33.6265 34.8157 25.5705C34.8157 17.5146 37.8685 15.9235 48.0804 15.9235C46.1509 15.6019 52.1 17.1293 52.1 25.5705C52.1 33.2078 48.1608 37.6293 47.2765 37.6293Z"
          fill="black"
        />
      </g>
      <defs>
        <filter
          id={filterId}
          x="-2.47955e-05"
          y="-2.47955e-05"
          width="63.2"
          height="51.945"
          filterUnits="userSpaceOnUse"
          colorInterpolationFilters="sRGB"
        >
          <feFlood floodOpacity="0" result="BackgroundImageFix" />
          <feBlend mode="normal" in="SourceGraphic" in2="BackgroundImageFix" result="shape" />
          <feGaussianBlur stdDeviation="5.55" result="effect1_foregroundBlur" />
        </filter>
      </defs>
    </svg>
  );
}

export default Iconmark;
