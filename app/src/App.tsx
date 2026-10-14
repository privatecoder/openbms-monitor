import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Cable, X } from "lucide-react";
import { Sidebar } from "./views/Sidebar";
import { MaintBanner } from "./views/MaintBanner";
import { NameDialog } from "./views/NameDialog";
import { Firmware } from "./views/Firmware";
import { Dashboard } from "./views/Dashboard";
import { PackDetail } from "./views/PackDetail";
import { Cells } from "./views/Cells";
import { Diagnosis } from "./views/Diagnosis";
import { ParametersView, showStoredParameters } from "./views/Parameters";
import { History } from "./views/History";
import { Installations } from "./views/Installations";
import { HelpProvider } from "./help";
import { useLiveData } from "./store";
import { useMaint } from "./maintStore";
import { api, isDemo, type Bus } from "./api";
import { defaultName, type Monitor } from "./maint";
import i18n from "./i18n";

const demoPack = isDemo ? new URLSearchParams(location.search).get("pack") : null;
const demoView = isDemo ? new URLSearchParams(location.search).get("view") : null;
// views listed in the sidebar only while "show maintenance functions" is on
const MAINT_VIEWS = ["diag", "params", "firmware"];

/** „Andere Platine an Adresse 07 (Seriennummer …)?“: a serial read found another board than the place had. */
type BoardNotice = { address: number; serial: string; previous: string };

export default function App() {
  const { t } = useTranslation();
  const live = useLiveData();
  const maint = useMaint();
  // the installation drives everything stored (null: „Nur ansehen“); it cannot change while connected
  const scope = maint.installation;
  const [connected, setConnected] = useState(false);
  const [bus, setBus] = useState<Bus | null>(null);
  const [view, setView] = useState(demoView ?? "dashboard");
  const [flashing, setFlashing] = useState(false);
  const [boards, setBoards] = useState<BoardNotice[]>([]);
  // counts actions the user starts in the firmware view or the pack bus panel: the sidebar's wait for a busy port stops
  const [userActions, setUserActions] = useState(0);
  const userAction = () => setUserActions((n) => n + 1);
  // a screenshot link straight into a maintenance view (demoView) shows that view
  const [maintenance, setMaintenance] = useState(() => localStorage.getItem("maint.enabled") === "1" || MAINT_VIEWS.includes(demoView ?? ""));
  const [pack, setPack] = useState<number | null>(demoPack !== null ? Number(demoPack) : null);
  const current = pack !== null ? live.packs[pack] : undefined;
  // what the monitor connection is doing, for the sidebar: three distinct "not connected" texts
  const [monitor, setMonitor] = useState<Monitor>("offline");
  // the app stopped the monitor itself (firmware view, pack bus panel): "paused for maintenance", no lost connection
  const monitorStopped = () => { live.reset(); setConnected(false); setPack(null); setMonitor((m) => (m === "connected" ? "paused" : m)); };
  const onMonitorStopped = useRef(monitorStopped);
  onMonitorStopped.current = monitorStopped;
  useEffect(() => {
    const un = api.onMonitorStopped(() => onMonitorStopped.current());
    return () => { un.then((f) => f()); };
  }, []);

  const enableMaintenance = () => { localStorage.setItem("maint.enabled", "1"); setMaintenance(true); };
  // the banner's „Anzeigen“: the firmware view with the pack bus panel of that installation. Another installation than
  // the connected one ends the connection first: the choice and the connection always belong together.
  const show = async (id: string) => {
    await maint.switchTo(id);
    enableMaintenance(); setView("firmware"); setPack(null);
  };
  const packList = Object.keys(live.packs).map(Number).sort((a, b) => a - b);

  return (
    <HelpProvider>
      <div className="flex h-full bg-bg text-ink">
        <Sidebar
          userActions={userActions}
          connected={connected}
          paused={monitor === "paused"}
          maint={maint}
          packs={packList}
          lastUpdate={Math.max(0, ...Object.values(live.packs).map((p) => p.updated))}
          locked={flashing}
          maintenance={maintenance}
          setMaintenance={(v) => { localStorage.setItem("maint.enabled", v ? "1" : "0"); setMaintenance(v); if (!v && MAINT_VIEWS.includes(view)) setView("dashboard"); }}
          view={view}
          setView={(v) => { setView(v); setPack(null); }}
          onConnected={(found, b) => { live.setDevices(found); setBus(b); setConnected(true); setMonitor("connected"); setBoards([]); if (!isDemo) setPack(null); }}
          onDevices={live.addDevices}
          onSerials={live.setSerials}
          onBoard={(n) => setBoards((list) => [...list.filter((x) => x.address !== n.address), n])}
          onDisconnected={(lost) => { live.reset(); setConnected(false); setPack(null); setMonitor(lost ? "lost" : "offline"); }}
        />
        <main className="flex-1 overflow-y-auto px-10 py-10">
          <MaintBanner alerts={maint.alerts} failed={maint.alertsFailed} flashing={flashing} onChanged={() => void maint.loadLists()} onShow={(id) => void show(id)} />
          {boards.map((b) => (
            <div key={b.address} role="status" className="mb-3 flex items-start gap-3 rounded-md border border-discharge p-3 text-sm">
              <p className="flex-1">{t("inst.otherBoard", { pack: String(b.address).padStart(2, "0"), serial: b.serial, previous: b.previous })}</p>
              <button className="text-muted hover:text-ink" aria-label={t("inst.dismiss")} title={t("inst.dismiss")} onClick={() => setBoards((l) => l.filter((x) => x !== b))}><X className="h-4 w-4" /></button>
            </div>
          ))}
          {maint.naming && <NameDialog suggested={defaultName(maint.installations, i18n.t("maint.picker.base"))} onConfirm={maint.create} onCancel={() => maint.setNaming(false)} />}
          {view === "firmware" && maintenance ? (
            <Firmware connected={connected} maint={maint} onRunning={setFlashing} onBack={() => setView("dashboard")} onUserAction={userAction} onMonitorStopped={monitorStopped} />
          ) : view === "installations" ? (
            <Installations maint={maint} connected={connected}
              onParameters={(id) => { showStoredParameters(); void maint.switchTo(id).then(() => { enableMaintenance(); setView("params"); }); }} />
          ) : view === "cells" ? (
            <Cells />
          ) : view === "history" ? (
            <History installation={maint.current} connected={connected} bus={bus} packs={packList} />
          ) : view === "params" && maintenance ? (
            <ParametersView connected={connected} bus={bus} installation={maint.current} packs={packList} />
          ) : !connected ? (
            <div className="flex h-full max-w-md flex-col justify-center gap-3 text-lg text-muted"><Cable className="h-8 w-8" strokeWidth={1.5} aria-hidden />{t("dash.empty")}</div>
          ) : view === "diag" && maintenance ? (
            <Diagnosis packs={live.packs} scope={scope} />
          ) : current ? (
            <PackDetail p={current} bus={bus} multi={Object.keys(live.packs).length > 1} onBack={() => setPack(null)} scope={scope} packs={live.packs} />
          ) : (
            <Dashboard packs={live.packs} system={live.system} scope={scope} onOpen={setPack} />
          )}
        </main>
      </div>
    </HelpProvider>
  );
}
