import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { BatteryCharging, Database, Languages, LayoutDashboard, Moon, Network, Sun, Usb } from "lucide-react";
import { api, isDemo, type Bus, type DeviceInfo } from "../api";
import { Button } from "../components/ui/button";
import { InfoIcon } from "../help";
import { cn } from "../lib/utils";

const input = "h-9 w-full rounded-md border border-line bg-surface px-3 text-sm text-ink outline-none focus:border-charge";

export function Sidebar({ connected, lastUpdate, onConnected, onDisconnected, view, setView }: {
  connected: boolean;
  lastUpdate: number;
  onConnected: (found: [number, DeviceInfo][], site: string) => void;
  onDisconnected: () => void;
  view: string;
  setView: (v: string) => void;
}) {
  const { t, i18n } = useTranslation();
  const [kind, setKind] = useState<"tcp" | "serial">(() => (localStorage.getItem("conn.kind") as "tcp" | "serial") ?? "tcp");
  const [target, setTarget] = useState(() => localStorage.getItem("conn.target") ?? (isDemo ? "demo:4196" : ""));
  const [bus, setBus] = useState<Bus>(() => (localStorage.getItem("conn.bus") as Bus) ?? "can");
  const [ports, setPorts] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);
  const [dark, setDark] = useState(document.documentElement.classList.contains("dark"));

  useEffect(() => { api.listPorts().then(setPorts).catch(() => setPorts([])); }, [kind]);
  useEffect(() => { document.documentElement.classList.toggle("dark", dark); try { localStorage.setItem("theme", dark ? "dark" : "light"); } catch { /* ignore */ } }, [dark]);

  const connect = async () => {
    setBusy(true); setMsg(null);
    try {
      localStorage.setItem("conn.kind", kind); localStorage.setItem("conn.target", target); localStorage.setItem("conn.bus", bus);
      await api.connect(kind === "tcp" ? { kind, addr: target, bus } : { kind, path: target, bus });
      const found = await api.scan();
      setMsg(found.length ? t("conn.found", { count: found.length }) : t("conn.none"));
      onConnected(found, target);
      if (found.length) await api.startPolling(found.map(([a]) => a), 2000);
    } catch (e) {
      setMsg(String(e));
    } finally { setBusy(false); }
  };

  // Browser preview: connect straight away so the recorded system is visible.
  useEffect(() => { if (isDemo) connect(); }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const disconnect = async () => { await api.disconnect(); onDisconnected(); setMsg(null); };

  return (
    <aside className="flex w-64 shrink-0 flex-col border-r border-line bg-surface">
      <div className="flex items-center gap-2.5 px-5 pb-4 pt-6">
        <BatteryCharging className="h-6 w-6 text-ok" aria-hidden />
        <span className="font-display text-xl font-semibold">{t("app")}</span>
      </div>
      <nav className="space-y-1 px-3">
        {[["dashboard", LayoutDashboard], ["cells", Database]].map(([id, Icon]) => {
          const I = Icon as typeof LayoutDashboard;
          return (
            <button key={id as string} onClick={() => setView(id as string)}
              className={cn("flex w-full items-center gap-2 rounded-md px-3 py-2", view === id ? "bg-sunken font-medium text-ink" : "text-muted hover:bg-sunken/60 hover:text-ink")}>
              <I className="h-4 w-4" />{t(`nav.${id}`)}
            </button>
          );
        })}
      </nav>
      <div className="mt-6 space-y-3 border-t border-line px-5 pt-5">
        <div className="flex items-center justify-between text-sm">
          <span className="flex items-center gap-1.5 font-medium">{t("conn.title")}<InfoIcon id="topic.connection" /></span>
          <span className={cn("flex items-center gap-1.5", connected ? "text-charge" : "text-muted")}>
            <span className="relative flex h-2 w-2">
              {connected && lastUpdate > 0 && <span key={lastUpdate} className="ping-once absolute inset-0 rounded-full bg-charge" />}
              <span className={cn("relative h-2 w-2 rounded-full", connected ? "bg-charge" : "border border-muted")} />
            </span>
            {connected ? t("conn.connected") : t("conn.offline")}
          </span>
        </div>
        <div className="grid grid-cols-2 gap-1 rounded-md bg-sunken p-1">
          {([["tcp", Network], ["serial", Usb]] as const).map(([k, I]) => (
            <button key={k} onClick={() => setKind(k)}
              className={cn("flex items-center justify-center gap-1.5 rounded py-1.5 text-sm", kind === k ? "bg-surface text-ink shadow-sm" : "text-muted hover:text-ink")}>
              <I className="h-3.5 w-3.5" />{t(`conn.${k}`)}
            </button>
          ))}
        </div>
        {kind === "tcp" ? (
          <input className={input} value={target} onChange={(e) => setTarget(e.target.value)} placeholder="192.168.1.10:4196" aria-label={t("conn.host")} />
        ) : (
          <select className={input} value={target} onChange={(e) => setTarget(e.target.value)} aria-label={t("conn.port")}>
            <option value="">{t("conn.port")}</option>
            {ports.map((p) => <option key={p}>{p}</option>)}
          </select>
        )}
        <label className="block">
          <span className="mb-1 flex items-center gap-1.5 text-sm text-muted">{t("conn.bus")}<InfoIcon id="topic.buses" /></span>
          <select className={input} value={bus} onChange={(e) => setBus(e.target.value as Bus)}>
            <option value="can">{t("conn.busCan")}</option>
            <option value="pack">{t("conn.busPack")}</option>
          </select>
        </label>
        <div className="flex gap-2">
          <Button variant="primary" className="flex-1" disabled={busy || !target} onClick={connect}>
            {busy ? t("conn.scanning") : connected ? t("conn.reconnect") : t("conn.connect")}
          </Button>
          {connected && <Button variant="ghost" onClick={disconnect}>{t("conn.disconnect")}</Button>}
        </div>
        {msg && <p className="text-sm text-muted">{msg}</p>}
        {isDemo && <p className="text-sm text-discharge">{t("conn.demo")}</p>}
      </div>
      <div className="mt-auto flex items-center gap-1 border-t border-line px-3 py-3">
        <Button variant="ghost" size="sm" onClick={() => i18n.changeLanguage(i18n.language === "de" ? "en" : "de")}>
          <Languages className="h-4 w-4" />{i18n.language.toUpperCase()}
        </Button>
        <Button variant="ghost" size="icon" onClick={() => setDark(!dark)} aria-label="Theme">
          {dark ? <Sun className="h-4 w-4" /> : <Moon className="h-4 w-4" />}
        </Button>
      </div>
    </aside>
  );
}
