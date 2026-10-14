import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { RefreshCw } from "lucide-react";
import { api, type ControlEndpoint, type Installation } from "../api";
import { errorKey, setupEndpoints, setupFields } from "../maint";
import { Button } from "../components/ui/button";

const input = "h-9 rounded-md border border-line bg-surface px-3 text-sm text-ink outline-none focus:border-charge disabled:opacity-50";

/** A port select that also shows a stored port that is not plugged in (it must not vanish from the form). */
function PortSelect({ value, ports, onChange, onRefresh, label, disabled }: { value: string; ports: string[]; onChange: (v: string) => void; onRefresh: () => void; label: string; disabled: boolean }) {
  const { t } = useTranslation();
  return (
    <span className="flex items-center gap-1">
      <select className={input} value={value} disabled={disabled} onChange={(e) => onChange(e.target.value)} aria-label={label}>
        <option value="">{t("conn.port")}</option>
        {value && !ports.includes(value) && <option value={value}>{t("maint.setup.notPlugged", { port: value })}</option>}
        {ports.map((p) => <option key={p} value={p}>{p}</option>)}
      </select>
      <Button variant="ghost" size="icon" disabled={disabled} onClick={onRefresh} aria-label={t("fw.refreshPorts")} title={t("fw.refreshPorts")}><RefreshCw className="h-4 w-4" /></Button>
    </span>
  );
}

/** The kind of the RS485-1/2 endpoint: USB only for now; flashing over a network gateway needs its bench test first
 *  (design: maintenance mode, "Transport for remote use"). */
export function PackKindSelect({ disabled }: { disabled: boolean }) {
  const { t } = useTranslation();
  return (
    <select className={input} value="serial" disabled={disabled} onChange={() => {}} aria-label={t("maint.setup.rs485Kind")}>
      <option value="serial">{t("conn.serial")}</option>
      <option value="tcp" disabled>{t("conn.tcp")}</option>
    </select>
  );
}

/** The maintenance parts of an installation: the CAN socket (reads and halts the master) and RS485-1/2 (packs are
 *  flashed there). Name, archive and delete are in the installations view; the master's serial number is stored by the
 *  app at the first flash. */
export function MaintSetup({ installation, onSaved, onCancel, disabled }: {
  installation: Installation;
  onSaved: (inst: Installation) => void;
  onCancel: () => void;
  disabled: boolean;
}) {
  const { t } = useTranslation();
  const [f, setF] = useState(() => setupFields(installation));
  const edit = (patch: Partial<typeof f>) => setF((x) => ({ ...x, ...patch }));
  const [ports, setPorts] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const loadPorts = () => { api.listPorts().then(setPorts).catch(() => setPorts([])); };
  useEffect(loadPorts, []);

  const can: ControlEndpoint | null = !f.canValue.trim() ? null : f.canKind === "tcp" ? { kind: "tcp", addr: f.canValue.trim() } : { kind: "serial", path: f.canValue };

  const save = async () => {
    setError(null); setSaving(true);
    try { onSaved(await api.maintSaveInstallation({ ...installation, control: can, endpoints: setupEndpoints(installation, f.pack) })); }
    catch (e) { const k = errorKey(e); setError(t(k.key, k.params)); }
    finally { setSaving(false); }
  };

  return (
    <div className="space-y-4 rounded-md border border-line p-3 text-sm">
      <div className="space-y-1">
        <p className="font-medium">{t("maint.setup.can")}</p>
        <div className="flex flex-wrap items-center gap-3">
          <select className={input} value={f.canKind} disabled={disabled} onChange={(e) => edit({ canKind: e.target.value as ControlEndpoint["kind"], canValue: "" })} aria-label={t("maint.setup.can")}>
            <option value="serial">{t("conn.serial")}</option>
            <option value="tcp">{t("conn.tcp")}</option>
          </select>
          {f.canKind === "tcp"
            ? <input className={input} value={f.canValue} disabled={disabled} onChange={(e) => edit({ canValue: e.target.value })} placeholder="192.168.1.10:4196" aria-label={t("conn.host")} />
            : <PortSelect value={f.canValue} ports={ports} onRefresh={loadPorts} onChange={(v) => edit({ canValue: v })} label={t("conn.port")} disabled={disabled} />}
        </div>
        <p className="text-muted">{t("maint.setup.controlHint")}</p>
      </div>

      <div className="space-y-1">
        <p className="font-medium">{t("maint.setup.rs485")}</p>
        <div className="flex flex-wrap items-center gap-3">
          <PackKindSelect disabled={disabled} />
          <PortSelect value={f.pack} ports={ports} onRefresh={loadPorts} onChange={(v) => edit({ pack: v })} label={t("maint.setup.rs485")} disabled={disabled} />
        </div>
        <p className="text-muted">{t("maint.setup.rs485Hint")}</p>
      </div>
      {installation.master_serial && <p className="text-muted">{t("maint.setup.serial", { serial: installation.master_serial })}</p>}

      <div className="flex flex-wrap gap-2">
        <Button variant="primary" size="sm" disabled={saving || disabled} onClick={save}>{t("maint.setup.save")}</Button>
        <Button variant="secondary" size="sm" disabled={saving} onClick={onCancel}>{t("maint.setup.cancel")}</Button>
      </div>
      {error && <p className="text-alarm">{error}</p>}
    </div>
  );
}
