import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { api, type ControlEndpoint, type MaintOverview } from "../api";
import { endpointKey, errorKey } from "../maint";
import { Button } from "../components/ui/button";

const input = "h-9 rounded-md border border-line bg-surface px-3 text-sm text-ink outline-none focus:border-charge";

export function MaintSetup({ overview, onSaved }: { overview: MaintOverview; onSaved: (o: MaintOverview) => void }) {
  const { t } = useTranslation();
  const inst = overview.installation;
  const [name, setName] = useState(inst.name);
  const [kind, setKind] = useState<ControlEndpoint["kind"]>(inst.control?.kind ?? "serial");
  const [value, setValue] = useState(inst.control ? (inst.control.kind === "tcp" ? inst.control.addr : inst.control.path) : "");
  const [ports, setPorts] = useState<string[]>([]);
  const [endpoints, setEndpoints] = useState<string[]>(inst.endpoints);
  const [packPort, setPackPort] = useState("");
  const [error, setError] = useState<string | null>(null);
  useEffect(() => { api.listPorts().then(setPorts).catch(() => setPorts([])); }, []);

  const addPackPort = () => {
    const key = endpointKey({ kind: "serial", path: packPort, bus: "pack" });
    if (packPort && !endpoints.includes(key)) setEndpoints([...endpoints, key]);
    setPackPort("");
  };

  const save = async () => {
    setError(null);
    const control: ControlEndpoint | null = !value ? null : kind === "tcp" ? { kind, addr: value } : { kind, path: value };
    try {
      await api.maintSaveInstallation({ ...inst, name, control, endpoints });
      onSaved(await api.maintOverview(inst.id));
    } catch (e) { const k = errorKey(e); setError(t(k.key, k.params)); }
  };

  return (
    <div className="space-y-3 text-sm">
      <label className="flex items-center gap-3">{t("maint.setup.name")}<input className={input} value={name} onChange={(e) => setName(e.target.value)} /></label>
      <div className="flex flex-wrap items-center gap-3">
        {t("maint.setup.control")}
        <select className={input} value={kind} onChange={(e) => { setKind(e.target.value as ControlEndpoint["kind"]); setValue(""); }}>
          <option value="serial">{t("conn.serial")}</option>
          <option value="tcp">{t("conn.tcp")}</option>
        </select>
        {kind === "tcp"
          ? <input className={input} value={value} onChange={(e) => setValue(e.target.value)} placeholder="192.168.1.10:4196" />
          : <select className={input} value={value} onChange={(e) => setValue(e.target.value)}><option value="">{t("conn.port")}</option>{ports.map((p) => <option key={p}>{p}</option>)}</select>}
      </div>
      <p className="text-muted">{t("maint.setup.controlHint")}</p>
      <div className="space-y-1">
        <p>{t("maint.setup.endpoints")}</p>
        {endpoints.length === 0 && <p className="text-muted">–</p>}
        {endpoints.map((ep) => (
          <p key={ep} className="flex items-center gap-2"><span className="font-mono text-xs">{ep}</span>
            <Button variant="ghost" size="sm" onClick={() => setEndpoints(endpoints.filter((x) => x !== ep))}>{t("maint.setup.remove")}</Button></p>
        ))}
        <div className="flex flex-wrap items-center gap-3">
          {t("maint.setup.packPort")}
          <select className={input} value={packPort} onChange={(e) => setPackPort(e.target.value)}><option value="">{t("conn.port")}</option>{ports.map((p) => <option key={p}>{p}</option>)}</select>
          <Button variant="ghost" size="sm" disabled={!packPort} onClick={addPackPort}>{t("maint.setup.add")}</Button>
        </div>
      </div>
      <Button variant="ghost" size="sm" onClick={save}>{t("maint.setup.save")}</Button>
      {error && <p className="text-alarm">{error}</p>}
    </div>
  );
}
