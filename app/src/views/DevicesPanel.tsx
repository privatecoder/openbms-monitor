import { useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Pencil, Plus, Trash2 } from "lucide-react";
import { Button } from "../components/ui/button";
import { InfoIcon } from "../help";
import { useSettings } from "../installData";
import { errorKey } from "../maint";
import { runExclusive } from "../exclusive";
import {
  BALANCER_DEFAULTS, WHENS, addBalancer, assignPacks, assignedNow, editBalancer, fromForm, liveDefs, packsOf, parseDevices, removeBalancer, summary, toForm,
  valuesOf, type BalancerForm, type Devices,
} from "../devices";

const input = "h-8 rounded-md border border-line bg-surface px-2 text-sm text-ink outline-none focus:border-charge disabled:opacity-50";
const packNo = (a: number) => String(a).padStart(2, "0");

/** „Geräte“ on the installation card: external active balancers with their manual settings and which pack has one.
 *  Every change is stored with its time (`devices.ts`), so the recording before keeps the settings of its time. */
export function DevicesPanel({ installation, places }: { installation: string; places: number[] }) {
  const { t } = useTranslation();
  const { settings, loaded, save } = useSettings(installation);
  const devices = parseDevices(settings.devices);
  const defs = liveDefs(devices);
  // the known places and every pack that ever had an assignment
  const packs = [...new Set([...places, ...Object.keys(devices.assign).map(Number)])].sort((a, b) => a - b);
  const [editing, setEditing] = useState<{ id: string | null; form: BalancerForm } | null>(null);
  const [removing, setRemoving] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  // one save at a time: a second click before the answer would add the definition again on the updated list
  const saving = useRef(false);
  const [busy, setBusy] = useState(false);

  const store = async (next: Devices) => {
    setError(null);
    const e = await save("devices", next);
    if (e) { const k = errorKey(e); setError(t("dev.failed", { e: t(k.key, k.params) })); }
    return !e;
  };
  const submit = () => runExclusive(saving, async () => {
    if (!editing) return;
    const v = fromForm(editing.form);
    if (!v) { setError(t("dev.invalid")); return; }
    const now = Date.now();
    const next = editing.id ? editBalancer(devices, editing.id, editing.form.name, v, now) : addBalancer(devices, editing.form.name, v, now)[0];
    setBusy(true);
    try {
      if (await store(next)) setEditing(null);
    } finally {
      setBusy(false);
    }
  });
  const name = (n: string) => n || t("dev.unnamed");
  const removingDef = defs.find((d) => d.id === removing);
  const removingPacks = removingDef ? packsOf(devices, removingDef.id, packs) : [];

  return (
    <div className="space-y-2 text-sm">
      <div className="flex items-center gap-2">
        <h3 className="font-medium">{t("dev.title")}</h3>
        <InfoIcon id="topic.balancers" />
        {!editing && <Button size="sm" variant="ghost" disabled={!loaded || busy} onClick={() => { setError(null); setEditing({ id: null, form: toForm("", BALANCER_DEFAULTS) }); }}><Plus className="h-4 w-4" />{t("dev.add")}</Button>}
      </div>
      <p className="max-w-3xl text-muted">{t("dev.intro")}</p>

      {defs.length === 0 && !editing && <p className="text-muted">{t("dev.none")}</p>}
      {defs.length > 0 && (
        <ul className="space-y-1">
          {defs.map((d) => (
            <li key={d.id} className="flex flex-wrap items-center gap-x-3 gap-y-1">
              <span className="font-medium">{name(d.name)}</span>
              <span className="text-muted">{t(`dev.kind.${d.kind}`)} · {summary(valuesOf(d), t)}</span>
              <Button size="sm" variant="ghost" title={t("dev.edit")} aria-label={t("dev.edit")} disabled={!!editing || busy}
                onClick={() => { setError(null); setEditing({ id: d.id, form: toForm(d.name, valuesOf(d)) }); }}><Pencil className="h-4 w-4" /></Button>
              <Button size="sm" variant="ghost" title={t("dev.remove")} aria-label={t("dev.remove")} onClick={() => setRemoving(d.id)}><Trash2 className="h-4 w-4" /></Button>
            </li>
          ))}
        </ul>
      )}

      {removingDef && (
        <div className="space-y-2 rounded-md border border-alarm/50 p-3">
          <p>{removingPacks.length ? t("dev.confirm", { name: name(removingDef.name), packs: removingPacks.map(packNo).join(", "), count: removingPacks.length }) : t("dev.confirmNone", { name: name(removingDef.name) })}</p>
          <div className="flex gap-2">
            <Button size="sm" variant="primary" onClick={() => void store(removeBalancer(devices, removingDef.id, packs, Date.now())).then((ok) => ok && setRemoving(null))}>{t("dev.yes")}</Button>
            <Button size="sm" variant="secondary" onClick={() => setRemoving(null)}>{t("dev.cancel")}</Button>
          </div>
        </div>
      )}

      {editing && <Editor form={editing.form} saving={busy} onChange={(form) => setEditing({ ...editing, form })} onSave={() => void submit()} onCancel={() => { setEditing(null); setError(null); }} />}

      {defs.length > 0 && (
        <div className="space-y-1.5 pt-1">
          <div className="flex flex-wrap items-center gap-2">
            <h4 className="font-medium">{t("dev.assign.title")}</h4>
            {packs.length > 0 && (
              <select className={input} value="" aria-label={t("dev.assign.all")}
                onChange={(e) => { if (e.target.value) void store(assignPacks(devices, packs, e.target.value === "-" ? null : e.target.value, Date.now())); }}>
                <option value="">{t("dev.assign.all")}</option>
                {defs.map((d) => <option key={d.id} value={d.id}>{name(d.name)}</option>)}
                <option value="-">{t("dev.assign.none")}</option>
              </select>
            )}
          </div>
          <p className="text-muted">{t("dev.assign.hint")}</p>
          {packs.length === 0 ? <p className="text-muted">{t("dev.assign.noPacks")}</p> : (
            <div className="grid grid-cols-[repeat(auto-fill,minmax(13rem,1fr))] gap-x-4 gap-y-1">
              {packs.map((p) => (
                <label key={p} className="flex items-center gap-2">
                  <span className="w-16 tabular-nums text-muted">Pack {packNo(p)}</span>
                  <select className={`${input} min-w-0 flex-1`} value={assignedNow(devices, p)?.id ?? ""}
                    onChange={(e) => void store(assignPacks(devices, [p], e.target.value || null, Date.now()))}>
                    <option value="">{t("dev.assign.none")}</option>
                    {defs.map((d) => <option key={d.id} value={d.id}>{name(d.name)}</option>)}
                  </select>
                </label>
              ))}
            </div>
          )}
        </div>
      )}
      {error && <p className="text-alarm">{error}</p>}
    </div>
  );
}

function Editor({ form, saving, onChange, onSave, onCancel }: { form: BalancerForm; saving: boolean; onChange: (f: BalancerForm) => void; onSave: () => void; onCancel: () => void }) {
  const { t } = useTranslation();
  const field = (k: Exclude<keyof BalancerForm, "when">, label: string, placeholder?: string, wide = false) => (
    <label className="flex flex-col gap-0.5">
      <span className="text-muted">{label}</span>
      <input className={`${input} ${wide ? "w-48" : "w-28"}`} value={form[k]} placeholder={placeholder} inputMode={k === "name" ? undefined : "decimal"} disabled={saving}
        onChange={(e) => onChange({ ...form, [k]: e.target.value })} />
    </label>
  );
  return (
    // while saving every field is fixed: an edit after the click would not be in what is being saved
    <form className="space-y-2 rounded-md border border-line p-3" onSubmit={(e) => { e.preventDefault(); onSave(); }}>
      <p className="font-medium">{t("dev.kind.active-balancer")}</p>
      <div className="flex flex-wrap items-end gap-3">
        {field("name", t("dev.f.name"), t("dev.f.namePh"), true)}
        {field("trigger_mv", t("dev.f.trigger"))}
        {field("stop_mv", t("dev.f.stop"), t("dev.f.stopPh"))}
        {field("start_mv", t("dev.f.start"), t("dev.f.startPh"))}
        <label className="flex flex-col gap-0.5">
          <span className="text-muted">{t("dev.f.when")}</span>
          <select className={input} value={form.when} disabled={saving} onChange={(e) => onChange({ ...form, when: e.target.value as BalancerForm["when"] })}>
            {WHENS.map((w) => <option key={w} value={w}>{t(`dev.when.${w}`)}</option>)}
          </select>
        </label>
        {field("max_a", t("dev.f.maxA"))}
      </div>
      <div className="flex gap-2">
        <Button size="sm" variant="primary" type="submit" disabled={saving}>{t("dev.save")}</Button>
        {/* while saving the draft stays: a late success must not close a newer one */}
        <Button size="sm" variant="ghost" type="button" disabled={saving} onClick={onCancel}>{t("dev.cancel")}</Button>
      </div>
    </form>
  );
}
