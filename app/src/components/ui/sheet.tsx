import { Dialog as D } from "radix-ui";
import { X } from "lucide-react";
import { cn } from "../../lib/utils";
import type { ComponentProps, ReactNode } from "react";

export const Sheet = D.Root;

export function SheetContent({ className, title, children, ...props }: ComponentProps<typeof D.Content> & { title: ReactNode }) {
  return (
    <D.Portal>
      <D.Overlay className="anim-overlay fixed inset-0 z-40 bg-ink/20" />
      <D.Content
        className={cn("anim-sheet fixed inset-y-0 right-0 z-50 flex w-[28rem] max-w-full flex-col border-l border-line bg-surface text-ink shadow-2xl outline-none", className)}
        {...props}
      >
        <div className="flex items-center justify-between border-b border-line px-6 py-4">
          <D.Title className="font-display text-xl font-semibold">{title}</D.Title>
          <D.Close className="rounded-md p-1 text-muted hover:bg-sunken" aria-label="Close">
            <X className="h-4 w-4" />
          </D.Close>
        </div>
        <D.Description className="sr-only">{typeof title === "string" ? title : "Help"}</D.Description>
        <div className="flex-1 overflow-y-auto px-6 py-5">{children}</div>
      </D.Content>
    </D.Portal>
  );
}
