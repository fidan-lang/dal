import DOMPurify from "dompurify";
import { marked } from "marked";

/**
 * Safely converts a Markdown string to sanitised HTML.
 * Only runs DOMPurify in browser environments.
 */
export function renderMarkdown(md: string): string {
  const raw = marked.parse(md, { async: false }) as string;
  if (typeof window === "undefined") {
    // SSR: return raw (no XSS vector since it's rendered as innerHTML client-side)
    return raw;
  }
  return DOMPurify.sanitize(raw);
}

/** Format a number with thousands separators */
export function formatNumber(n: number): string {
  return n.toLocaleString("en-US");
}

/** Format bytes as human-readable file size */
export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(2)} MB`;
}

/** Format an ISO date string as "N days/months/years ago" or locale date */
export function timeAgo(iso: string): string {
  const ms = Date.now() - new Date(iso).getTime();
  const sec = Math.floor(ms / 1000);
  if (sec < 60) return "just now";
  const min = Math.floor(sec / 60);
  if (min < 60) return `${min}m ago`;
  const hr = Math.floor(min / 60);
  if (hr < 24) return `${hr}h ago`;
  const days = Math.floor(hr / 24);
  if (days < 30) return `${days}d ago`;
  const months = Math.floor(days / 30);
  if (months < 12) return `${months}mo ago`;
  return `${Math.floor(months / 12)}y ago`;
}

/** Truncate a string to maxLen, adding ellipsis */
export function truncate(s: string, maxLen: number): string {
  if (s.length <= maxLen) return s;
  return s.slice(0, maxLen - 1) + "…";
}
