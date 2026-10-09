import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

export const fmt = (v: number | null | undefined, digits: number, unit = "") =>
  v === null || v === undefined || Number.isNaN(v)
    ? "–"
    : `${v.toLocaleString(undefined, { minimumFractionDigits: digits, maximumFractionDigits: digits }).replace("-", "\u2212")}${unit ? ` ${unit}` : ""}`;
