import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { api, type ControlEndpoint, type Installation, type MaintOverview } from "../api";
import { deleteAllowed, errorKey, nameCheck, packPortOf, sameAdapter, sectorNoteKey, setupCheck, setupEndpoints, setupFields } from "../maint";
import { oneAdapterOf, setOneAdapter } from "../maintStore";
import { Button } from "../components/ui/button";

const input = "h-9 rounded-md border border-line bg-surface px-3 text-sm text-ink outline-none focus:border-charge disabled:opacity-50";

const fieldsOf = (inst: Installation) => setupFields(inst, oneAdapterOf(inst.id));
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

/** The installation's setup: its name and two ports, the CAN socket (control channel for the master) and
 *  RS485-1/2 (measure and flash packs); deleting it and resolving a note on sector 0x23000. Without `overview` the
 *  empty form of „Neue Anlage …“ (name required), which creates the installation on save.
 *  `disabled`: a panel action runs; the form can be neither changed nor saved meanwhile. */
export function MaintSetup({ overview, installations, onSaved, onCreated, onDeleted, onCancel, disabled }: {
  overview: MaintOverview | null;
  /** all installations: the name must differ from theirs */
  installations: Installation[];
  onSaved: (o: MaintOverview) => void;
  onCreated?: (inst: Installation) => void;
  onDeleted?: (inst: Installation) => void;
  onCancel?: () => void;
  disabled: boolean;
}) {
  const { t } = useTranslation();
  const creating = !overview;
  const id = overview?.installation.id ?? "";
  const [f, setF] = useState(() => fieldsOf(overview?.installation ?? blank));
  // the RS485-1/2 port the fields were (re)loaded with: tells "set meanwhile" from "cleared by the user"
  const loadedPack = useRef(f.pack);
  const load = (inst: Installation) => { const x = fieldsOf(inst); loadedPack.current = x.pack; setF(x); };
  // the user changed a field: a late reload of the stored installation no longer overwrites the form
  const touched = useRef(false);
  const edit = (patch: Partial<typeof f>) => { touched.current = true; setF((x) => ({ ...x, ...patch })); };
  const [ports, setPorts] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  // the inline confirmation of delete / resolve
  const [confirming, setConfirming] = useState<"delete" | "resolve" | null>(null);
  useEffect(() => { api.listPorts().then(setPorts).catch(() => setPorts([])); }, []);
  // the form starts from the stored installation, not from a possibly older overview in memory
  useEffect(() => {
    if (!id) return;
    let live = true;
    api.maintOverview(id).then((o) => { if (live && !touched.current) load(o.installation); }).catch(() => {});
    return () => { live = false; };
  }, [id]);

  const can: ControlEndpoint | null = !f.canValue.trim() ? null : f.canKind === "tcp" ? { kind: "tcp", addr: f.canValue.trim() } : { kind: "serial", path: f.canValue };
  const same = sameAdapter(can, f.pack);

  const save = async () => {
    setError(null);
    const bad = nameCheck(f.name, installations, creating ? null : id);
    if (bad) { setError(t(bad)); return; }
    setSaving(true);
    try {
      if (creating) {
        const refused = setupCheck({ can, pack: f.pack, oneAdapter: f.oneAdapter, storedPack: null });
        if (refused) { setError(t(refused)); return; }
        const created = await api.maintCreateInstallation(f.name.trim(), can, setupEndpoints(blank, can, f.pack));
        setOneAdapter(created.id, same && f.oneAdapter);
        onCreated?.(created);
        return;
      }
      // guard: build the list from the installation as stored now, so a port added meanwhile is never dropped
      const stored = (await api.maintOverview(id)).installation;
      const storedOne = oneAdapterOf(id);
      const refused = setupCheck({ can, pack: f.pack, oneAdapter: f.oneAdapter, storedPack: packPortOf(stored, storedOne), loadedPack: loadedPack.current });
      // a pack port was set meanwhile (e.g. by another view): show the stored fields instead of the stale ones
      if (refused === "maint.setup.err.packSetMeanwhile") { touched.current = false; load(stored); }
      if (refused) { setError(t(refused)); return; }
      await api.maintSaveInstallation({ ...stored, name: f.name, control: can, endpoints: setupEndpoints(stored, can, f.pack, storedOne) });
      setOneAdapter(id, same && f.oneAdapter);
      onSaved(await api.maintOverview(id));
    } catch (e) { const k = errorKey(e); setError(t(k.key, k.params)); }
    finally { setSaving(false); }
  };
  // delete and resolve: the backend re-checks open work under the port lock
  const act = async (op: () => Promise<void>) => {
    setError(null); setSaving(true);
    try { await op(); setConfirming(null); } catch (e) { const k = errorKey(e); setError(t(k.key, k.params)); } finally { setSaving(false); }
  };
  const remove = () => act(async () => { await api.maintDeleteInstallation(id); if (overview) onDeleted?.(overview.installation); });
  const resolve = () => act(async () => onSaved(await api.maintResolveSectorNote(id)));
  const note = overview?.sector_note ?? null;
  const noteKey = note ? sectorNoteKey(note) : null;

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
        <div className="flex flex-wrap items-center gap-3">
          {/* network for RS485-1/2 follows with stage 2: shown, not selectable */}
          <select className={input} value="serial" disabled={disabled} onChange={() => {}} aria-label={t("maint.setup.rs485")}>
            <option value="serial">{t("conn.serial")}</option>
            <option value="tcp" disabled>{t("conn.tcp")}</option>
          </select>
          <PortSelect value={f.pack} ports={ports} onChange={(v) => edit({ pack: v })} label={t("conn.port")} disabled={disabled} />
        </div>
        <p className="text-muted">{t("maint.setup.networkLater")}</p>
      </div>

      {same && (
        <label className="flex items-center gap-2"><input type="checkbox" checked={f.oneAdapter} disabled={disabled} onChange={(e) => edit({ oneAdapter: e.target.checked })} />{t("maint.setup.oneAdapter")}</label>
      )}
      <div className="flex flex-wrap gap-2">
        <Button variant="primary" size="sm" disabled={saving || disabled} onClick={save}>{t(creating ? "maint.setup.create" : "maint.setup.save")}</Button>
        {creating && onCancel && <Button variant="secondary" size="sm" disabled={saving} onClick={onCancel}>{t("maint.setup.cancel")}</Button>}
      </div>
      {error && <p className="text-alarm">{error}</p>}

      {/* a note on 0x23000 is never discarded silently: resolved explicitly, then archived */}
      {note && (
        <div className="space-y-2 border-t border-line pt-3">
          <p className="text-discharge">{t("maint.setup.sectorNote", { note: noteKey ? t(noteKey) : note })}</p>
          {confirming === "resolve" ? (
            <div className="space-y-2">
              <p>{t("maint.setup.resolveConfirm")}</p>
              <div className="flex gap-2">
                <Button variant="primary" size="sm" disabled={saving || disabled} onClick={resolve}>{t("maint.setup.resolveYes")}</Button>
                <Button variant="secondary" size="sm" disabled={saving} onClick={() => setConfirming(null)}>{t("maint.setup.cancel")}</Button>
              </div>
            </div>
          ) : <Button variant="secondary" size="sm" disabled={saving || disabled} onClick={() => setConfirming("resolve")}>{t("maint.setup.resolve")}</Button>}
        </div>
      )}

      {overview && (
        <div className="space-y-2 border-t border-line pt-3">
          {confirming === "delete" ? (
            <div className="space-y-2">
              <p className="font-medium">{t("maint.setup.deleteConfirm", { name: overview.installation.name })}</p>
              <div className="flex gap-2">
                <Button variant="primary" size="sm" disabled={saving || disabled} onClick={remove}>{t("maint.setup.deleteYes")}</Button>
                <Button variant="secondary" size="sm" disabled={saving} onClick={() => setConfirming(null)}>{t("maint.setup.cancel")}</Button>
              </div>
            </div>
          ) : <Button variant="secondary" size="sm" disabled={saving || disabled || !deleteAllowed(overview)} onClick={() => setConfirming("delete")}>{t("maint.setup.delete")}</Button>}
          {!deleteAllowed(overview) && <p className="text-muted">{t("maint.setup.deleteBlocked")}</p>}
        </div>
      )}
    </div>
  );
}
