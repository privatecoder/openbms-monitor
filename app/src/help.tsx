import { createContext, useContext, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import Markdown from "react-markdown";
import { Info } from "lucide-react";
import { Popover, PopoverContent, PopoverTrigger } from "./components/ui/popover";
import { Sheet, SheetContent } from "./components/ui/sheet";

// Help content: ../help/<lang>/<id>.md with a small front matter (title, short, related).
const files = import.meta.glob("../../help/*/*.md", { query: "?raw", import: "default", eager: true }) as Record<string, string>;

interface Entry { title: string; short: string; body: string; related: string[] }

const entries: Record<string, Record<string, Entry>> = {};
for (const [path, raw] of Object.entries(files)) {
  const m = path.match(/help\/([a-z]{2})\/(.+)\.md$/);
  if (!m) continue;
  const [, lang, id] = m;
  const fm = raw.match(/^---\n([\s\S]*?)\n---\n?([\s\S]*)$/);
  const meta: Record<string, string> = {};
  if (fm) for (const line of fm[1].split("\n")) {
    const i = line.indexOf(":");
    if (i > 0) meta[line.slice(0, i).trim()] = line.slice(i + 1).trim();
  }
  (entries[lang] ??= {})[id] = {
    title: meta.title ?? id,
    short: meta.short ?? "",
    body: fm ? fm[2].trim() : raw,
    related: (meta.related ?? "").split(",").map((s) => s.trim()).filter(Boolean),
  };
}

export function useHelpEntry(id: string): Entry | undefined {
  const { i18n } = useTranslation();
  return entries[i18n.language]?.[id] ?? entries.en?.[id];
}

const HelpCtx = createContext<(id: string) => void>(() => {});
export const useOpenHelp = () => useContext(HelpCtx);

function HelpPage({ id, onOpen }: { id: string; onOpen: (id: string) => void }) {
  const { t } = useTranslation();
  const e = useHelpEntry(id);
  if (!e) return <p className="text-sm text-muted">{t("help.missing")}</p>;
  return (
    <article className="space-y-4 text-sm leading-relaxed">
      <p className="text-ink/80">{e.short}</p>
      <div className="space-y-3 [&_li]:ml-4 [&_li]:list-disc [&_strong]:font-semibold [&_table]:w-full [&_td]:border-t [&_td]:border-line [&_td]:py-1 [&_td]:pr-3 [&_th]:pr-3 [&_th]:text-left [&_th]:font-medium [&_code]:rounded [&_code]:bg-sunken [&_code]:px-1">
        <Markdown>{e.body}</Markdown>
      </div>
      {e.related.length > 0 && (
        <div>
          <div className="mb-2 text-sm text-muted">{t("help.related")}</div>
          <div className="flex flex-wrap gap-2">
            {e.related.map((r) => <RelatedLink key={r} id={r} onOpen={onOpen} />)}
          </div>
        </div>
      )}
    </article>
  );
}

function RelatedLink({ id, onOpen }: { id: string; onOpen: (id: string) => void }) {
  const e = useHelpEntry(id);
  return (
    <button onClick={() => onOpen(id)} className="rounded-md border border-line px-2.5 py-1 text-sm hover:bg-sunken">
      {e?.title ?? id}
    </button>
  );
}

export function HelpProvider({ children }: { children: ReactNode }) {
  const [id, setId] = useState<string | null>(null);
  const e = useHelpEntry(id ?? "");
  const { t } = useTranslation();
  return (
    <HelpCtx.Provider value={setId}>
      {children}
      <Sheet open={id !== null} onOpenChange={(o) => !o && setId(null)}>
        <SheetContent title={e?.title ?? t("help.title")}>{id && <HelpPage id={id} onOpen={setId} />}</SheetContent>
      </Sheet>
    </HelpCtx.Provider>
  );
}

/** Info icon with a popover (click, keyboard, touch) and a link to the full help page. */
export function InfoIcon({ id, className }: { id: string; className?: string }) {
  const { t } = useTranslation();
  const e = useHelpEntry(id);
  const open = useOpenHelp();
  return (
    <Popover>
      <PopoverTrigger
        aria-label={e?.title ?? id}
        className={`inline-flex rounded-full text-muted/70 hover:text-ink ${className ?? ""}`}
      >
        <Info className="h-3.5 w-3.5" />
      </PopoverTrigger>
      <PopoverContent>
        <div className="mb-1 font-display text-base font-semibold">{e?.title ?? id}</div>
        <p className="text-ink/80">{e?.short ?? t("help.missing")}</p>
        {e && (
          <button onClick={() => open(id)} className="mt-3 text-sm font-medium text-charge underline-offset-2 hover:underline">
            {t("help.more")}
          </button>
        )}
      </PopoverContent>
    </Popover>
  );
}
