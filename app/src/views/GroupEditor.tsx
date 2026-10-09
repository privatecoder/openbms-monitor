import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Plus, Trash2 } from "lucide-react";
import { Sheet, SheetContent } from "../components/ui/sheet";
import { Button } from "../components/ui/button";
import { splitEvenly, type Group } from "../groups";
import { cn } from "../lib/utils";

const input = "h-9 rounded-md border border-line bg-surface px-3 text-sm text-ink outline-none focus:border-charge";

export function GroupEditor({ open, onOpenChange, groups, packs, onSave }: {
  open: boolean; onOpenChange: (o: boolean) => void; groups: Group[]; packs: number[]; onSave: (g: Group[]) => void;
}) {
  const { t } = useTranslation();
  const [draft, setDraft] = useState<Group[]>(groups);
  const [count, setCount] = useState(3);
  const perGroup = Math.ceil(packs.length / Math.max(1, count));
  useEffect(() => { if (open) setDraft(groups.length ? groups : []); }, [open, groups]);

  const owner = (a: number) => draft.findIndex((g) => g.packs.includes(a));
  const toggle = (gi: number, a: number) => setDraft((d) => d.map((g, i) => {
    if (i === gi) return { ...g, packs: g.packs.includes(a) ? g.packs.filter((x) => x !== a) : [...g.packs, a].sort((x, y) => x - y) };
    return { ...g, packs: g.packs.filter((x) => x !== a) };
  }));
  const rename = (gi: number, name: string) => setDraft((d) => d.map((g, i) => (i === gi ? { ...g, name } : g)));
  const remove = (gi: number) => setDraft((d) => d.filter((_, i) => i !== gi));
  const add = () => setDraft((d) => [...d, { name: `${t("groups.defaultName")} ${d.length + 1}`, packs: [] }]);

  return (
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent title={t("groups.title")}>
        <div className="space-y-6">
          <p className="text-sm text-muted">{t("groups.intro")}</p>
          <div className="flex flex-wrap items-center gap-2 rounded-md bg-sunken p-3">
            <span className="text-sm">{t("groups.splitPre")}</span>
            <input type="number" min={1} max={Math.max(1, packs.length)} value={count} onChange={(e) => setCount(Math.max(1, Math.min(packs.length || 1, Number(e.target.value) || 1)))}
              className={cn(input, "w-16 text-center")} aria-label={t("groups.splitPre")} />
            <span className="text-sm">{t("groups.splitPost", { count: perGroup })}</span>
            <Button size="sm" className="ml-auto" onClick={() => setDraft(splitEvenly(packs, perGroup, t("groups.defaultName")))}>{t("groups.split")}</Button>
          </div>

          {draft.map((g, gi) => (
            <div key={gi} className="space-y-2">
              <div className="flex items-center gap-2">
                <input className={cn(input, "flex-1 font-medium")} value={g.name} onChange={(e) => rename(gi, e.target.value)} aria-label={t("groups.name")} />
                <Button variant="ghost" size="icon" onClick={() => remove(gi)} aria-label={t("groups.remove")}><Trash2 className="h-4 w-4" /></Button>
              </div>
              <div className="flex flex-wrap gap-1.5">
                {packs.map((a) => {
                  const o = owner(a), mine = o === gi;
                  return (
                    <button key={a} onClick={() => toggle(gi, a)} aria-pressed={mine}
                      title={o >= 0 && !mine ? draft[o].name : undefined}
                      className={cn("h-8 w-10 rounded-md border font-display text-sm font-semibold",
                        mine ? "border-ink bg-ink text-surface" : o >= 0 ? "border-line text-muted/50 line-through" : "border-line hover:bg-sunken")}>
                      {String(a).padStart(2, "0")}
                    </button>
                  );
                })}
              </div>
            </div>
          ))}

          <Button variant="ghost" onClick={add}><Plus className="h-4 w-4" />{t("groups.add")}</Button>

          <div className="flex items-center gap-2 border-t border-line pt-4">
            <Button variant="primary" onClick={() => { onSave(draft.filter((g) => g.name.trim() || g.packs.length)); onOpenChange(false); }}>{t("groups.save")}</Button>
            <Button variant="ghost" onClick={() => onOpenChange(false)}>{t("groups.cancel")}</Button>
            {draft.length > 0 && <Button variant="ghost" className="ml-auto" onClick={() => setDraft([])}>{t("groups.clear")}</Button>}
          </div>
        </div>
      </SheetContent>
    </Sheet>
  );
}
