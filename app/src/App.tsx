import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Cable } from "lucide-react";
import { Sidebar } from "./views/Sidebar";
import { MaintBanner } from "./views/MaintBanner";
import { NameDialog } from "./views/NameDialog";
import { Firmware } from "./views/Firmware";
import { Dashboard } from "./views/Dashboard";
import { PackDetail } from "./views/PackDetail";
import { Cells } from "./views/Cells";
import { Diagnosis } from "./views/Diagnosis";
import { ParametersView } from "./views/Parameters";
import { History } from "./views/History";
import { HelpProvider } from "./help";
import { useLiveData } from "./store";
import { useMaint } from "./maintStore";
import { api, isDemo, type Bus } from "./api";
import type { Monitor } from "./maint";

const demoPack = isDemo ? new URLSearchParams(location.search).get("pack") : null;
const demoView = isDemo ? new URLSearchParams(location.search).get("view") : null;
// views listed in the sidebar only while "show maintenance functions" is on
const MAINT_VIEWS = ["diag", "params", "firmware"];

export default function App() {
  const { t } = useTranslation();
  const live = useLiveData();
  const maint = useMaint();
  const [connected, setConnected] = useState(false);
  const [site, setSite] = useState("");
  const [bus, setBus] = useState<Bus | null>(null);
  const [view, setView] = useState(demoView ?? "dashboard");
  const [flashing, setFlashing] = useState(false);
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

  // the banner's „Anzeigen“: the firmware view with the pack bus panel
  const openPanel = () => { localStorage.setItem("maint.enabled", "1"); setMaintenance(true); setView("firmware"); setPack(null); };
  const endpointText = (ep: { kind: string; addr?: string; path?: string }) => ep.addr ?? ep.path ?? "";

  return (
    <HelpProvider>
      <div className="flex h-full bg-bg text-ink">
        <Sidebar
          userActions={userActions}
          connected={connected}
          paused={monitor === "paused"}
          site={site}
          packs={Object.keys(live.packs).map(Number)}
          lastUpdate={Math.max(0, ...Object.values(live.packs).map((p) => p.updated))}
          locked={flashing}
          maintenance={maintenance}
          setMaintenance={(v) => { localStorage.setItem("maint.enabled", v ? "1" : "0"); setMaintenance(v); if (!v && MAINT_VIEWS.includes(view)) setView("dashboard"); }}
          view={view}
          setView={(v) => { setView(v); setPack(null); }}
          onConnected={(found, s, b) => { live.setDevices(found); setSite(s); setBus(b); setConnected(true); setMonitor("connected"); if (!isDemo) setPack(null); }}
          onDevices={live.addDevices}
          onSerials={live.setSerials}
          onEndpoint={(ep) => maint.attach(ep)}
          onDisconnected={(lost) => { live.reset(); setConnected(false); setPack(null); setMonitor(lost ? "lost" : "offline"); }}
        />
        <main className="flex-1 overflow-y-auto px-10 py-10">
          <MaintBanner alerts={maint.alerts} failed={maint.alertsFailed} flashing={flashing} unassigned={maint.unassigned} shownName={maint.current?.name ?? null}
            onAssign={() => maint.unassigned && maint.assign(maint.unassigned)} onChanged={() => void maint.loadLists()}
            onShow={(id) => { if (id !== maint.installation) void maint.switchTo(id); openPanel(); }} />
          {maint.naming && <NameDialog key={maint.naming.suggested + endpointText(maint.naming.endpoint)} endpoint={maint.naming.endpoint} suggested={maint.naming.suggested} onConfirm={maint.confirmNaming} onCancel={maint.cancelNaming} />}
          {view === "firmware" && maintenance ? (
            <Firmware connected={connected} maint={maint} onRunning={setFlashing} onBack={() => setView("dashboard")} onUserAction={userAction} onMonitorStopped={monitorStopped} />
          ) : view === "cells" ? (
            <Cells />
          ) : view === "history" ? (
            <History site={site} connected={connected} bus={bus} packs={Object.keys(live.packs).map(Number).sort((a, b) => a - b)} />
          ) : view === "params" && maintenance ? (
            <ParametersView connected={connected} bus={bus} site={site} packs={Object.keys(live.packs).map(Number).sort((a, b) => a - b)} />
          ) : !connected ? (
            <div className="flex h-full max-w-md flex-col justify-center gap-3 text-lg text-muted"><Cable className="h-8 w-8" strokeWidth={1.5} aria-hidden />{t("dash.empty")}</div>
          ) : view === "diag" && maintenance ? (
            <Diagnosis packs={live.packs} site={site} />
          ) : current ? (
            <PackDetail p={current} bus={bus} multi={Object.keys(live.packs).length > 1} onBack={() => setPack(null)} site={site} packs={live.packs} />
          ) : (
            <Dashboard packs={live.packs} system={live.system} site={site} onOpen={setPack} />
          )}
        </main>
      </div>
    </HelpProvider>
  );
}
