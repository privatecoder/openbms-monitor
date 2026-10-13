import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { api, type ControlEndpoint, type Installation } from "../api";
import { errorKey, nameCheck, setupEndpoints, setupFields } from "../maint";
import { Button } from "../components/ui/button";

const input = "h-9 rounded-md border border-line bg-surface px-3 text-sm text-ink outline-none focus:border-charge disabled:opacity-50";

// the empty form of „Neue Anlage …“
const blank: Installation = { id: "", name: "", endpoints: [], control: null };

/** A port select that also shows a stored port that is not plugged in (it must not vanish from the form). */
function PortSelect({ value, ports, onChange, label, disabled }: { value: string; ports: string[]; onChange: (v: string) => void; label: string; disabled: boolean }) {
  const { t } = useTranslation();
  return (
    <select className={input} value={value} disabled={disabled} onChange={(e) => onChange(e.target.value)} aria-label={label}>
      <option value="">{t("conn.port")}</option>
      {value && !ports.includes(value) && <option value={value}>{t("maint.setup.notPlugged", { port: value })}</option>}
      {ports.map((p) => <option key={p} value={p}>{p}</option>)}
    </select>
  );
}

/** An installation of the address book: its name, the CAN socket (reads and halts the master) and RS485-1/2 (packs
 *  are flashed there); delete. Without `installation` the empty form of „Neue Anlage …“, which creates it on save.
 *  The master's serial number is stored by the app at the first flash. */
export function MaintSetup({ installation, installations, onSaved, onDeleted, onCancel, disabled }: {
  installation: Installation | null;
  installations: Installation[];
  onSaved: (inst: Installation) => void;
  onDeleted?: (inst: Installation) => void;
  onCancel: () => void;
  disabled: boolean;
}) {
  const { t } = useTranslation();
  const creating = !installation;
  const [f, setF] = useState(() => setupFields(installation ?? blank));
  const edit = (patch: Partial<typeof f>) => setF((x) => ({ ...x, ...patch }));
  const [ports, setPorts] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);
  useEffect(() => { api.listPorts().then(setPorts).catch(() => setPorts([])); }, []);

  const can: ControlEndpoint | null = !f.canValue.trim() ? null : f.canKind === "tcp" ? { kind: "tcp", addr: f.canValue.trim() } : { kind: "serial", path: f.canValue };

  const save = async () => {
    setError(null);
    const bad = nameCheck(f.name, installations, installation?.id ?? null);
    if (bad) { setError(t(bad)); return; }
    setSaving(true);
    try {
      if (!installation) onSaved(await api.maintCreateInstallation(f.name.trim(), can, setupEndpoints(blank, f.pack)));
      else onSaved(await api.maintSaveInstallation({ ...installation, name: f.name, control: can, endpoints: setupEndpoints(installation, f.pack) }));
    } catch (e) { const k = errorKey(e); setError(t(k.key, k.params)); }
    finally { setSaving(false); }
  };
  const remove = async () => {
    if (!installation) return;
    setError(null); setSaving(true);
    try { await api.maintDeleteInstallation(installation.id); onDeleted?.(installation); }
    catch (e) { const k = errorKey(e); setError(t(k.key, k.params)); }
    finally { setSaving(false); }
  };

  return (
    <div className="space-y-4 rounded-md border border-line p-3 text-sm">
      {creating && <p className="font-medium">{t("maint.setup.createTitle")}</p>}
      <label className="flex flex-wrap items-center gap-3">{t("maint.setup.name")}<input className={input} value={f.name} disabled={disabled} onChange={(e) => edit({ name: e.target.value })} /></label>

      <div className="space-y-1">
        <p className="font-medium">{t("maint.setup.can")}</p>
        <div className="flex flex-wrap items-center gap-3">
          <select className={input} value={f.canKind} disabled={disabled} onChange={(e) => edit({ canKind: e.target.value as ControlEndpoint["kind"], canValue: "" })} aria-label={t("maint.setup.can")}>
            <option value="serial">{t("conn.serial")}</option>
            <option value="tcp">{t("conn.tcp")}</option>
          </select>
          {f.canKind === "tcp"
            ? <input className={input} value={f.canValue} disabled={disabled} onChange={(e) => edit({ canValue: e.target.value })} placeholder="192.168.1.10:4196" aria-label={t("conn.host")} />
            : <PortSelect value={f.canValue} ports={ports} onChange={(v) => edit({ canValue: v })} label={t("conn.port")} disabled={disabled} />}
        </div>
        <p className="text-muted">{t("maint.setup.controlHint")}</p>
      </div>

      <div className="space-y-1">
        <p className="font-medium">{t("maint.setup.rs485")}</p>
        <PortSelect value={f.pack} ports={ports} onChange={(v) => edit({ pack: v })} label={t("maint.setup.rs485")} disabled={disabled} />
        <p className="text-muted">{t("maint.setup.rs485Hint")}</p>
      </div>
      {installation?.master_serial && <p className="text-muted">{t("maint.setup.serial", { serial: installation.master_serial })}</p>}

      <div className="flex flex-wrap gap-2">
        <Button variant="primary" size="sm" disabled={saving || disabled} onClick={save}>{t(creating ? "maint.setup.create" : "maint.setup.save")}</Button>
        <Button variant="secondary" size="sm" disabled={saving} onClick={onCancel}>{t("maint.setup.cancel")}</Button>
      </div>
      {error && <p className="text-alarm">{error}</p>}

      {installation && (
        <div className="space-y-2 border-t border-line pt-3">
          {confirmDelete ? (
            <div className="space-y-2">
              <p className="font-medium">{t("maint.setup.deleteConfirm", { name: installation.name })}</p>
              <div className="flex gap-2">
                <Button variant="primary" size="sm" disabled={saving || disabled} onClick={remove}>{t("maint.setup.deleteYes")}</Button>
                <Button variant="secondary" size="sm" disabled={saving} onClick={() => setConfirmDelete(false)}>{t("maint.setup.cancel")}</Button>
              </div>
            </div>
          ) : <Button variant="secondary" size="sm" disabled={saving || disabled} onClick={() => setConfirmDelete(true)}>{t("maint.setup.delete")}</Button>}
        </div>
      )}
    </div>
  );
}
