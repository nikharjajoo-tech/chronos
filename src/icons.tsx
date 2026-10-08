import type { ReactNode } from "react";

// Tiny inline icons; stroke uses currentColor.

type P = { size?: number };
const svg = (size: number, children: ReactNode) => (
  <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2.2} strokeLinecap="round" strokeLinejoin="round" aria-hidden>
    {children}
  </svg>
);

export const IconPlay = ({ size = 18 }: P) => (
  <svg width={size} height={size} viewBox="0 0 24 24" aria-hidden><path d="M8 5.5v13a1 1 0 0 0 1.5.86l10.4-6.5a1 1 0 0 0 0-1.72L9.5 4.64A1 1 0 0 0 8 5.5Z" fill="currentColor" /></svg>
);
export const IconPause = ({ size = 18 }: P) => (
  <svg width={size} height={size} viewBox="0 0 24 24" aria-hidden><rect x="6" y="5" width="4.2" height="14" rx="1.2" fill="currentColor" /><rect x="13.8" y="5" width="4.2" height="14" rx="1.2" fill="currentColor" /></svg>
);
export const IconReset = ({ size = 14 }: P) => svg(size, <><path d="M4 12a8 8 0 1 0 2.4-5.7" /><path d="M4 4v4.5h4.5" /></>);
export const IconSkip = ({ size = 14 }: P) => svg(size, <><path d="M5 5l8 7-8 7z" fill="currentColor" /><path d="M18 5v14" /></>);
export const IconHide = ({ size = 12 }: P) => svg(size, <><path d="M3 12s3.5-6.5 9-6.5 9 6.5 9 6.5-3.5 6.5-9 6.5S3 12 3 12Z" /><path d="M4 20 20 4" /></>);
export const IconMenu = ({ size = 12 }: P) => (
  <svg width={size} height={size} viewBox="0 0 24 24" aria-hidden><circle cx="5" cy="12" r="2.3" fill="currentColor" /><circle cx="12" cy="12" r="2.3" fill="currentColor" /><circle cx="19" cy="12" r="2.3" fill="currentColor" /></svg>
);
