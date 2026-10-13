import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { api, type ControlEndpoint, type Installation, type MaintOverview } from "../api";
import { errorKey, packPortOf, sameAdapter, setupCheck, setupEndpoints } from "../maint";
import { oneAdapterOf, setOneAdapter } from "../maintStore";
import { Button } from "../components/ui/button";

const input = "h-9 rounded-md border border-line bg-surface px-3 text-sm text-ink outline-none focus:border-charge disabled:opacity-50";

/** The form's fields from a stored installation. */
function fieldsOf(inst: Installation) {
  const c = inst.control;
  return {
    name: inst.name,
    canKind: (c?.kind ?? "serial") as ControlEndpoint["kind"],
    canValue: c ? (c.kind === "tcp" ? c.addr : c.path) : "",
    pack: packPortOf(inst, oneAdapterOf(inst.id)) ?? "",
    oneAdapter: oneAdapterOf(inst.id),
  };
}

/** A port select that also shows a stored port that is not plugged in (it must not vanish from the form). */
function PortSelect({ value, ports, onChange, label }: { value: string; ports: string[]; onChange: (v: string) => void; label: string }) {
  const { t } = useTranslation();
  return (
    <select className={input} value={value} onChange={(e) => onChange(e.target.value)} aria-label={label}>
      <option value="">{t("conn.port")}</option>
      {value && !ports.includes(value) && <option value={value}>{t("maint.setup.notPlugged", { port: value })}</option>}
      {ports.map((p) => <option key={p} value={p}>{p}</option>)}
    </select>
  );
}

/** The installation's setup: its name and two ports, the CAN socket (control channel for the master) and
 *  RS485-1/2 (measure and flash packs). */
export function MaintSetup({ overview, onSaved }: { overview: MaintOverview; onSaved: (o: MaintOverview) => void }) {
  const { t } = useTranslation();
  const id = overview.installation.id;
  const [f, setF] = useState(() => fieldsOf(overview.installation));
  // the user changed a field: a late reload of the stored installation no longer overwrites the form
  const touched = useRef(false);
  const edit = (patch: Partial<typeof f>) => { touched.current = true; setF((x) => ({ ...x, ...patch })); };
  const [ports, setPorts] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  useEffect(() => { api.listPorts().then(setPorts).catch(() => setPorts([])); }, []);
  // the form starts from the stored installation, not from a possibly older overview in memory
  useEffect(() => {
    let live = true;
    api.maintOverview(id).then((o) => { if (live && !touched.current) setF(fieldsOf(o.installation)); }).catch(() => {});
    return () => { live = false; };
  }, [id]);

  const can: ControlEndpoint | null = !f.canValue.trim() ? null : f.canKind === "tcp" ? { kind: "tcp", addr: f.canValue.trim() } : { kind: "serial", path: f.canValue };
  const same = sameAdapter(can, f.pack);

  const save = async () => {
    setError(null); setSaving(true);
    try {
      // guard: build the list from the installation as stored now, so a port added meanwhile is never dropped
      const stored = (await api.maintOverview(id)).installation;
      const storedOne = oneAdapterOf(id);
      const refused = setupCheck({ can, pack: f.pack, oneAdapter: f.oneAdapter, storedPack: packPortOf(stored, storedOne) });
      if (refused) { setError(t(refused)); return; }
      await api.maintSaveInstallation({ ...stored, name: f.name, control: can, endpoints: setupEndpoints(stored, can, f.pack, storedOne) });
      setOneAdapter(id, same && f.oneAdapter);
      onSaved(await api.maintOverview(id));
    } catch (e) { const k = errorKey(e); setError(t(k.key, k.params)); }
    finally { setSaving(false); }
  };

  return (
    <div className="space-y-4 rounded-md border border-line p-3 text-sm">
      <label className="flex flex-wrap items-center gap-3">{t("maint.setup.name")}<input className={input} value={f.name} onChange={(e) => edit({ name: e.target.value })} /></label>

      <div className="space-y-1">
        <p className="font-medium">{t("maint.setup.can")}</p>
        <div className="flex flex-wrap items-center gap-3">
          <select className={input} value={f.canKind} onChange={(e) => edit({ canKind: e.target.value as ControlEndpoint["kind"], canValue: "" })} aria-label={t("maint.setup.can")}>
            <option value="serial">{t("conn.serial")}</option>
            <option value="tcp">{t("conn.tcp")}</option>
          </select>
          {f.canKind === "tcp"
            ? <input className={input} value={f.canValue} onChange={(e) => edit({ canValue: e.target.value })} placeholder="192.168.1.10:4196" aria-label={t("conn.host")} />
            : <PortSelect value={f.canValue} ports={ports} onChange={(v) => edit({ canValue: v })} label={t("conn.port")} />}
        </div>
        <p className="text-muted">{t("maint.setup.controlHint")}</p>
      </div>

      <div className="space-y-1">
        <p className="font-medium">{t("maint.setup.rs485")}</p>
        <div className="flex flex-wrap items-center gap-3">
          {/* network for RS485-1/2 follows with stage 2: shown, not selectable */}
          <select className={input} value="serial" onChange={() => {}} aria-label={t("maint.setup.rs485")}>
            <option value="serial">{t("conn.serial")}</option>
            <option value="tcp" disabled>{t("conn.tcp")}</option>
          </select>
          <PortSelect value={f.pack} ports={ports} onChange={(v) => edit({ pack: v })} label={t("conn.port")} />
        </div>
        <p className="text-muted">{t("maint.setup.networkLater")}</p>
      </div>

      {same && (
        <label className="flex items-center gap-2"><input type="checkbox" checked={f.oneAdapter} onChange={(e) => edit({ oneAdapter: e.target.checked })} />{t("maint.setup.oneAdapter")}</label>
      )}
      <Button variant="primary" size="sm" disabled={saving} onClick={save}>{t("maint.setup.save")}</Button>
      {error && <p className="text-alarm">{error}</p>}
    </div>
  );
}
