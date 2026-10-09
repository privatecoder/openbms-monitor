import { cn } from "../../lib/utils";
import type { HTMLAttributes } from "react";

/** A plain panel; hierarchy comes from spacing and type, not from shadows. */
export function Panel({ className, ...props }: HTMLAttributes<HTMLDivElement>) {
  return <div className={cn("rounded-md border border-line bg-surface", className)} {...props} />;
}

export function Label({ className, ...props }: HTMLAttributes<HTMLDivElement>) {
  return <div className={cn("flex items-center gap-1.5 text-sm text-muted", className)} {...props} />;
}
