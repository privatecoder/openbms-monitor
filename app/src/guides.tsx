import { useState } from "react";
import { useTranslation } from "react-i18next";
import { BookOpen } from "lucide-react";
import { Button } from "./components/ui/button";
import { Popover, PopoverContent, PopoverTrigger } from "./components/ui/popover";
import { helpTitle, useOpenHelp } from "./help";
import { cn } from "./lib/utils";

// The step-by-step guides (content in tools/guides.py, pages help/<lang>/guide.*.md) and the one place that decides
// where each appears. Views only put <GuideButton place="…"/>; title and content stay with the guide.
export const GUIDE_PLACES = {
  "guide.firmware": ["firmware", "packBus"],
  "guide.paramMulti": ["parameters"],
  "guide.setup": ["installations", "connection"],
} as const satisfies Record<string, readonly string[]>;

export type GuideId = keyof typeof GUIDE_PLACES;
export type GuidePlace = (typeof GUIDE_PLACES)[GuideId][number];

// narrow rows (the sidebar's connection head) get the icon alone, with the label as tooltip
const ICON_ONLY: ReadonlySet<GuidePlace> = new Set<GuidePlace>(["connection"]);

/** The guides of a place, in the order of the mapping. */
export const guidesAt = (place: GuidePlace): GuideId[] =>
  (Object.keys(GUIDE_PLACES) as GuideId[]).filter((id) => (GUIDE_PLACES[id] as readonly string[]).includes(place));

/** One guide opens directly; several are offered to choose from. */
export function guideChoice(place: GuidePlace): { kind: "none" } | { kind: "open"; id: GuideId } | { kind: "pick"; ids: GuideId[] } {
  const ids = guidesAt(place);
  return ids.length === 0 ? { kind: "none" } : ids.length === 1 ? { kind: "open", id: ids[0] } : { kind: "pick", ids };
}

/** „Anleitung“: opens the guide of this place in the help sheet (or a short list when there are several). */
export function GuideButton({ place }: { place: GuidePlace }) {
  const { t, i18n } = useTranslation();
  const openHelp = useOpenHelp();
  const [open, setOpen] = useState(false);
  const choice = guideChoice(place);
  if (choice.kind === "none") return null;
  const icon = ICON_ONLY.has(place);
  const title = choice.kind === "open" ? t("guide.hint", { title: helpTitle(choice.id, i18n.language) }) : t("guide.pick");
  const button = (onClick?: () => void) => (
    <Button variant="ghost" size={icon ? "icon" : "sm"} onClick={onClick} title={title} aria-label={icon ? t("guide.button") : undefined}
      className={cn("font-sans font-normal tracking-normal", icon ? "h-6 w-6" : "h-7 px-2")}>
      <BookOpen className="h-4 w-4" aria-hidden />{!icon && t("guide.button")}
    </Button>
  );
  if (choice.kind === "open") return button(() => openHelp(choice.id));
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>{button()}</PopoverTrigger>
      <PopoverContent>
        <div className="mb-2 font-display text-base font-semibold">{t("guide.pick")}</div>
        <ul className="space-y-1">
          {choice.ids.map((id) => (
            <li key={id}>
              <button type="button" onClick={() => { setOpen(false); openHelp(id); }} className="text-left text-sm font-medium text-charge underline-offset-2 hover:underline">
                {helpTitle(id, i18n.language)}
              </button>
            </li>
          ))}
        </ul>
      </PopoverContent>
    </Popover>
  );
}
