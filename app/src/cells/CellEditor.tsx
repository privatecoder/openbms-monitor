import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Plus, Trash2 } from "lucide-react";
import { Sheet, SheetContent } from "../components/ui/sheet";
import { Button } from "../components/ui/button";
import { cellErrors } from "./validate";
import type { Basis, Cell, DeratingPoint, Provenance } from "./types";
import { cn } from "../lib/utils";

type Kind = "number" | "text" | "rate";
interface Field { path: string; kind: Kind; unit?: string }

/** Form fields in display order; every value path gets its own provenance entry. */
export const FIELDS: { group: string; fields: Field[] }[] = [
  { group: "capacity", fields: [{ path: "capacity.nominal_ah", kind: "number", unit: "Ah" }, { path: "capacity.nominal_energy_wh", kind: "number", unit: "Wh" }] },
  { group: "voltage", fields: [
    { path: "voltage.nominal_v", kind: "number", unit: "V" }, { path: "voltage.charge_cutoff_v", kind: "number", unit: "V" },
    { path: "voltage.discharge_cutoff_v", kind: "number", unit: "V" }, { path: "voltage.discharge_cutoff_low_temp_v", kind: "number", unit: "V" },
  ] },
  { group: "current", fields: [
    { path: "current.standard_charge", kind: "rate" }, { path: "current.standard_discharge", kind: "rate" },
    { path: "current.max_continuous_charge_a", kind: "number", unit: "A" }, { path: "current.max_continuous_discharge_a", kind: "number", unit: "A" },
  ] },
  { group: "temperature", fields: [
    { path: "temperature.charge_min_c", kind: "number", unit: "°C" }, { path: "temperature.charge_max_c", kind: "number", unit: "°C" },
    { path: "temperature.discharge_min_c", kind: "number", unit: "°C" }, { path: "temperature.discharge_max_c", kind: "number", unit: "°C" },
  ] },
];

const get = (o: unknown, path: string) => path.split(".").reduce<unknown>((x, k) => (x && typeof x === "object" ? (x as Record<string, unknown>)[k] : undefined), o);
function set(o: Record<string, unknown>, path: string, value: unknown) {
  const keys = path.split(".");
  let x = o;
  for (const k of keys.slice(0, -1)) x = (x[k] ??= {}) as Record<string, unknown>;
  if (value === undefined) delete x[keys.at(-1)!];
  else x[keys.at(-1)!] = value;
  // drop groups that became empty, so the entry never carries {}
  if (keys.length === 2 && Object.keys(o[keys[0]] as object).length === 0) delete o[keys[0]];
}
const today = () => new Date().toISOString().slice(0, 10);
const input = "h-8 w-full rounded-md border border-line bg-surface px-2 text-sm text-ink outline-none focus:border-charge";

export function CellEditor({ open, onOpenChange, initial, isNew, onSave }: {
  open: boolean; onOpenChange: (o: boolean) => void; initial: Cell; isNew: boolean; onSave: (c: Cell) => Promise<void> | void;
}) {
  const { t } = useTranslation();
  const [draft, setDraft] = useState<Cell>(initial);
  const [json, setJson] = useState("");
  const [mode, setMode] = useState<"form" | "json">("form");
  const [errors, setErrors] = useState<string[]>([]);
  useEffect(() => { if (open) { setDraft(structuredClone(initial)); setMode("form"); setErrors([]); } }, [open, initial]);

  const update = (fn: (c: Record<string, unknown>) => void) => setDraft((d) => { const c = structuredClone(d) as unknown as Record<string, unknown>; fn(c); return c as unknown as Cell; });
  const prov = (path: string): Provenance | undefined => draft.provenance?.[path];
  const setProv = (path: string, p: Provenance | undefined) => update((c) => {
    const all = { ...((c.provenance as Record<string, Provenance>) ?? {}) };
    if (p) all[path] = p; else delete all[path];
    c.provenance = all;
  });

  const toJson = () => { setJson(JSON.stringify(draft, null, 2)); setMode("json"); };
  const fromJson = (): Cell | null => {
    try { return JSON.parse(json) as Cell; } catch (e) { setErrors([String(e)]); return null; }
  };
  const save = async () => {
    const c = mode === "json" ? fromJson() : draft;
    if (!c) return;
    const next = { ...c, updated: today(), entry_version: isNew ? c.entry_version : (initial.entry_version ?? 0) + 1 };
    const errs = cellErrors(next);
    if (errs.length) { setErrors(errs); return; }
    try { await onSave(next); } catch (e) { setErrors([String(e instanceof Error ? e.message : e)]); return; }
    onOpenChange(false);
  };

  return (
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent title={isNew ? t("cells.newTitle") : t("cells.editTitle", { name: `${initial.manufacturer} ${initial.model}` })} className="w-[44rem]">
        <div className="space-y-5">
          <div className="flex rounded-md bg-sunken p-0.5 text-sm" role="group">
            {(["form", "json"] as const).map((m) => (
              <button key={m} aria-pressed={mode === m} onClick={() => (m === "json" ? toJson() : (() => { const c = fromJson(); if (c) { setDraft(c); setMode("form"); setErrors([]); } })())}
                className={cn("flex-1 rounded px-3 py-1", mode === m ? "bg-surface text-ink shadow-sm" : "text-muted hover:text-ink")}>{t(`cells.mode.${m}`)}</button>
            ))}
          </div>

          {mode === "json" ? (
            <textarea value={json} onChange={(e) => setJson(e.target.value)} spellCheck={false}
              className="h-[60vh] w-full rounded-md border border-line bg-surface p-3 font-mono text-xs text-ink outline-none focus:border-charge" />
          ) : (
            <>
              <section className="grid grid-cols-2 gap-3">
                {(["id", "manufacturer", "model", "variant"] as const).map((k) => (
                  <label key={k} className="text-sm">
                    <span className="mb-1 block text-muted">{t(`cells.f.${k}`)}</span>
                    <input className={input} value={(draft[k] as string) ?? ""} disabled={k === "id" && !isNew}
                      onChange={(e) => update((c) => { if (e.target.value) c[k] = e.target.value; else delete c[k]; })} />
                  </label>
                ))}
                <label className="text-sm">
                  <span className="mb-1 block text-muted">{t("cells.f.chemistry")}</span>
                  <select className={input} value={draft.chemistry} onChange={(e) => update((c) => { c.chemistry = e.target.value; })}>
                    {["LFP", "NMC", "LTO", "NA-ION", "OTHER"].map((x) => <option key={x}>{x}</option>)}
                  </select>
                </label>
                <label className="text-sm">
                  <span className="mb-1 block text-muted">{t("cells.f.source_type")}</span>
                  <select className={input} value={draft.datasheet.source_type} onChange={(e) => update((c) => { (c.datasheet as Record<string, unknown>).source_type = e.target.value; })}>
                    {["manufacturer_pdf", "manufacturer_web", "distributor_pdf"].map((x) => <option key={x} value={x}>{t(`cells.src.${x}`)}</option>)}
                  </select>
                </label>
                <label className="col-span-2 text-sm">
                  <span className="mb-1 block text-muted">{t("cells.f.title")}</span>
                  <input className={input} value={draft.datasheet.title} onChange={(e) => update((c) => { (c.datasheet as Record<string, unknown>).title = e.target.value; })} />
                </label>
                <label className="col-span-2 text-sm">
                  <span className="mb-1 block text-muted">{t("cells.f.url")}</span>
                  <input className={input} value={draft.datasheet.url} onChange={(e) => update((c) => { (c.datasheet as Record<string, unknown>).url = e.target.value; })} />
                </label>
              </section>

              {FIELDS.map((g) => (
                <section key={g.group} className="space-y-2">
                  <h3 className="font-display text-lg font-semibold">{t(`cells.g.${g.group}`)}</h3>
                  {g.fields.map((f) => <FieldRow key={f.path} f={f} value={get(draft, f.path)} prov={prov(f.path)}
                    onValue={(v) => update((c) => set(c, f.path, v))} onProv={(p) => setProv(f.path, p)} />)}
                </section>
              ))}

              <DeratingEditor draft={draft} prov={prov("charge_derating")} onProv={(p) => setProv("charge_derating", p)}
                onChange={(d) => update((c) => { if (d) c.charge_derating = d; else delete c.charge_derating; })} />
            </>
          )}

          {errors.length > 0 && (
            <div className="rounded-md border border-alarm/40 bg-alarm/10 p-3 text-sm text-alarm">
              <p className="mb-1 font-medium">{t("cells.invalid")}</p>
              <ul className="list-disc pl-5">{errors.slice(0, 12).map((e) => <li key={e}>{e}</li>)}</ul>
            </div>
          )}
          <div className="flex gap-2 border-t border-line pt-4">
            <Button variant="primary" onClick={save}>{t("cells.save")}</Button>
            <Button variant="ghost" onClick={() => onOpenChange(false)}>{t("groups.cancel")}</Button>
          </div>
        </div>
      </SheetContent>
    </Sheet>
  );
}

/** One value with its provenance: value, "verified" checkbox and note. */
function FieldRow({ f, value, prov, onValue, onProv }: { f: Field; value: unknown; prov?: Provenance; onValue: (v: unknown) => void; onProv: (p: Provenance | undefined) => void }) {
  const { t } = useTranslation();
  const num = (s: string) => (s.trim() === "" ? undefined : Number(s.replace(",", ".")));
  const rate = value as { value: number; basis: Basis } | undefined;
  return (
    <div className="grid grid-cols-[11rem_9rem_auto_1fr] items-center gap-2 text-sm">
      <span className="text-muted">{t(`cells.f.${f.path.split(".")[1]}`)}</span>
      {f.kind === "rate" ? (
        <span className="flex gap-1">
          <input className={input} inputMode="decimal" value={rate?.value ?? ""} onChange={(e) => { const v = num(e.target.value); onValue(v === undefined ? undefined : { value: v, basis: rate?.basis ?? "C" }); }} />
          <select className={cn(input, "w-14")} value={rate?.basis ?? "C"} onChange={(e) => rate && onValue({ ...rate, basis: e.target.value })}>
            <option>C</option><option>P</option>
          </select>
        </span>
      ) : (
        <span className="flex items-center gap-1">
          <input className={input} inputMode="decimal" value={(value as number | undefined) ?? ""} onChange={(e) => onValue(num(e.target.value))} />
          <span className="w-6 text-muted">{f.unit}</span>
        </span>
      )}
      <label className="flex items-center gap-1 text-muted" title={t("cells.verifiedHint")}>
        <input type="checkbox" checked={!!prov?.verified} onChange={(e) => onProv({ ...(prov ?? {}), verified: e.target.checked })} />{t("cells.verified")}
      </label>
      <input className={input} placeholder={t("cells.notePh")} value={prov?.note ?? ""}
        onChange={(e) => onProv(e.target.value || prov?.verified ? { ...(prov ?? { verified: false }), note: e.target.value || undefined } : undefined)} />
    </div>
  );
}

/** Derating table as editable rows (temperature point or band, optional SOC band, value). */
function DeratingEditor({ draft, prov, onProv, onChange }: { draft: Cell; prov?: Provenance; onProv: (p: Provenance | undefined) => void; onChange: (d: Cell["charge_derating"]) => void }) {
  const { t } = useTranslation();
  const d = draft.charge_derating;
  const num = (s: string) => (s.trim() === "" ? undefined : Number(s.replace(",", ".")));
  const setPoint = (i: number, k: keyof DeratingPoint, v: number | undefined) => {
    if (!d) return;
    const points = d.points.map((p, j) => { if (j !== i) return p; const n = { ...p }; if (v === undefined) delete n[k]; else n[k] = v; return n; });
    onChange({ ...d, points });
  };
  const cols: (keyof DeratingPoint)[] = ["temp_c", "temp_min_c", "temp_max_c", "soc_min_pct", "soc_max_pct", "value"];
  return (
    <section className="space-y-2">
      <h3 className="font-display text-lg font-semibold">{t("cells.g.derating")}</h3>
      <div className="flex flex-wrap items-center gap-3 text-sm">
        {d ? (
          <>
            <label className="flex items-center gap-1 text-muted">{t("cells.basis")}
              <select className={cn(input, "w-14")} value={d.basis} onChange={(e) => onChange({ ...d, basis: e.target.value as Basis })}><option>C</option><option>P</option></select>
            </label>
            <Button size="sm" variant="ghost" onClick={() => onChange(undefined)}>{t("cells.removeTable")}</Button>
          </>
        ) : (
          <Button size="sm" onClick={() => onChange({ basis: "C", points: [{ temp_c: 25, value: 0.5 }] })}><Plus className="h-4 w-4" />{t("cells.addTable")}</Button>
        )}
        <label className="flex items-center gap-1 text-muted"><input type="checkbox" checked={!!prov?.verified} onChange={(e) => onProv({ ...(prov ?? {}), verified: e.target.checked })} />{t("cells.verified")}</label>
        <input className={cn(input, "flex-1")} placeholder={d ? t("cells.tableNotePh") : t("cells.noTableNotePh")} value={prov?.note ?? ""}
          onChange={(e) => onProv(e.target.value || prov?.verified ? { ...(prov ?? { verified: false }), note: e.target.value || undefined } : undefined)} />
      </div>
      {d && (
        <table className="w-full text-sm">
          <thead><tr className="text-left text-xs text-muted">{cols.map((c) => <th key={c} className="px-1 font-normal">{t(`cells.p.${c}`)}</th>)}<th /></tr></thead>
          <tbody>
            {d.points.map((p, i) => (
              <tr key={i}>
                {cols.map((c) => <td key={c} className="px-1 py-0.5"><input className={input} inputMode="decimal" value={p[c] ?? ""} onChange={(e) => setPoint(i, c, num(e.target.value))} /></td>)}
                <td><Button variant="ghost" size="icon" aria-label={t("cells.removeRow")} onClick={() => onChange({ ...d, points: d.points.filter((_, j) => j !== i) })}><Trash2 className="h-4 w-4" /></Button></td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      {d && <Button size="sm" variant="ghost" onClick={() => onChange({ ...d, points: [...d.points, { value: 0 }] })}><Plus className="h-4 w-4" />{t("cells.addRow")}</Button>}
    </section>
  );
}
