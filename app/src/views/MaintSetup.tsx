import { useEffect, useState } from "react";
import { actLocked } from "../locks";
import { useTranslation } from "react-i18next";
import { RefreshCw } from "lucide-react";
import { accessHint } from "../access";
import { api, type ControlEndpoint, type Installation, type Pending } from "../api";
import { errorKey, portKey, refusalParams, setupEndpoints, setupFields, type PortKind } from "../maint";
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

/** Network (a gateway, Host:Port) or USB (a port) for one of the two ports. */
export function KindSelect({ value, onChange, label, disabled }: { value: PortKind; onChange: (k: PortKind) => void; label: string; disabled: boolean }) {
  const { t } = useTranslation();
  return (
    <select className={input} value={value} disabled={disabled} onChange={(e) => onChange(e.target.value as PortKind)} aria-label={label}>
      <option value="serial">{t("conn.serial")}</option>
      <option value="tcp">{t("conn.tcp")}</option>
    </select>
  );
}

/** One port: a gateway address or a USB port. */
export function PortInput({ kind, value, ports, onChange, onRefresh, label, disabled }: { kind: PortKind; value: string; ports: string[]; onChange: (v: string) => void; onRefresh: () => void; label: string; disabled: boolean }) {
  const { t } = useTranslation();
  return kind === "tcp"
    ? <input className={input} value={value} disabled={disabled} onChange={(e) => onChange(e.target.value)} placeholder="192.168.1.10:4196" aria-label={`${label}: ${t("conn.host")}`} />
    : <PortSelect value={value} ports={ports} onRefresh={onRefresh} onChange={onChange} label={label} disabled={disabled} />;
}

/** The ports of an installation (installations view): the CAN socket (the monitor on the CAN socket bus; reads and
 *  halts the master) and RS485-1/2 (the monitor on the pack bus; packs are flashed there), each over a network gateway
 *  or USB. The master's serial number is stored by the app at the first flash. */
export function MaintSetup({ installation, pending, markerOn, onSaved, onCancel, disabled }: {
  installation: Installation;
  /** the master's marker was last seen on: it is released over the CAN socket saved here */
  markerOn: boolean;
  /** the open bootloader records, to name the port of a refusal */
  pending: Pending[];
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
    try { onSaved(await api.maintSaveInstallation({ ...installation, control: can, endpoints: setupEndpoints(installation, portKey(f.packKind, f.packValue)) })); }
    catch (e) {
      const k = errorKey(e);
      setError(t(k.key, refusalParams(installation, pending, k.params)));
    }
    finally { setSaving(false); }
  };

  return (
    <div className="space-y-4 rounded-md border border-line p-3 text-sm">
      <div className="space-y-1">
        <p className="font-medium">{t("maint.setup.can")}</p>
        <div className="flex flex-wrap items-center gap-3">
          <KindSelect value={f.canKind} disabled={disabled} onChange={(k) => edit({ canKind: k, canValue: "" })} label={t("maint.setup.can")} />
          <PortInput kind={f.canKind} value={f.canValue} ports={ports} onRefresh={loadPorts} onChange={(v) => edit({ canValue: v })} label={t("maint.setup.can")} disabled={disabled} />
        </div>
        <p className="text-muted">{accessHint(t, "maint.setup.controlHint")}</p>
        {markerOn && <p className="text-discharge">{t("maint.setup.markerOn")}</p>}
      </div>

      <div className="space-y-1">
        <p className="font-medium">{t("maint.setup.rs485")}</p>
        <div className="flex flex-wrap items-center gap-3">
          <KindSelect value={f.packKind} disabled={disabled} onChange={(k) => edit({ packKind: k, packValue: "" })} label={t("maint.setup.rs485Kind")} />
          <PortInput kind={f.packKind} value={f.packValue} ports={ports} onRefresh={loadPorts} onChange={(v) => edit({ packValue: v })} label={t("maint.setup.rs485")} disabled={disabled} />
        </div>
        <p className="text-muted">{t("maint.setup.rs485Hint")}</p>
      </div>
      {installation.master_serial && <p className="text-muted">{t("maint.setup.serial", { serial: installation.master_serial })}</p>}

      <div className="flex flex-wrap gap-2">
        <Button variant="primary" size="sm" disabled={actLocked(saving, disabled)} onClick={save}>{t("maint.setup.save")}</Button>
        <Button variant="secondary" size="sm" disabled={actLocked(saving, false)} onClick={onCancel}>{t("maint.setup.cancel")}</Button>
      </div>
      {error && <p className="text-alarm">{error}</p>}
    </div>
  );
}
