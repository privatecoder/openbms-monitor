import { Popover as P } from "radix-ui";
import { cn } from "../../lib/utils";
import type { ComponentProps } from "react";

export const Popover = P.Root;
export const PopoverTrigger = P.Trigger;

export function PopoverContent({ className, ...props }: ComponentProps<typeof P.Content>) {
  return (
    <P.Portal>
      <P.Content
        sideOffset={6}
        collisionPadding={12}
        className={cn("anim-pop z-50 w-80 rounded-md border border-line bg-surface p-4 text-sm text-ink shadow-lg outline-none", className)}
        {...props}
      />
    </P.Portal>
  );
}
