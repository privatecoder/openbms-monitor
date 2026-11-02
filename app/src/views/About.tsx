import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import Markdown from "react-markdown";
import { BatteryCharging, BookOpen, Bug, ExternalLink, FolderOpen, GitBranch, Package, Scale, ScrollText } from "lucide-react";
import { api, type AboutLink, type AppInfo } from "../api";
import { Panel } from "../components/ui/card";
import { Button } from "../components/ui/button";
import changelog from "../../../CHANGELOG.md?raw";

/** The changelog without its title and intro: from the first version heading on. */
export const changelogBody = (raw: string) => {
  const i = raw.search(/^## /m);
  return i < 0 ? raw : raw.slice(i);
};

const LINKS: [AboutLink, typeof GitBranch][] = [["repo", GitBranch], ["releases", Package], ["docs", BookOpen], ["issues", Bug], ["license", Scale]];

export function About() {
  const { t, i18n } = useTranslation();
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => { api.appInfo().then(setInfo, (e) => setError(String(e))); }, []);
  const open = (which: AboutLink) => { api.openLink(which).catch((e) => setError(String(e))); };

  return (
    <div className="view-in mx-auto flex max-w-4xl flex-col gap-5 pb-8">
      <header className="flex items-center gap-4">
        <BatteryCharging className="h-12 w-12 shrink-0 text-ok" strokeWidth={1.5} aria-hidden />
        <div>
          <h1 className="font-display text-4xl font-semibold tracking-tight">{t("app")}</h1>
          <p className="mt-1 text-muted">
            {t("about.version", { version: info?.version ?? "…" })}{info?.version.includes("-") && <> · {t("about.prerelease")}</>}
          </p>
        </div>
      </header>
      <p className="max-w-3xl text-ink/80">{t("about.intro")}</p>
      {error && <p className="text-sm text-alarm">{error}</p>}

      <div className="flex flex-wrap gap-2">
        {LINKS.map(([id, I]) => (
          <Button key={id} size="sm" variant="ghost" onClick={() => open(id)} title={t(`about.link.${id}Hint`)}>
            <I className="h-4 w-4" />{t(`about.link.${id}`)}<ExternalLink className="h-3 w-3 text-muted" aria-hidden />
          </Button>
        ))}
      </div>

      <Panel className="flex flex-wrap items-center justify-between gap-3 p-4">
        <div className="min-w-0">
          <div className="font-medium">{t("about.data")}</div>
          <p className="text-sm text-muted">{t("about.dataHint")}</p>
          {info?.data_dir && <code className="mt-1 block break-all rounded bg-sunken px-1.5 py-0.5 text-xs">{info.data_dir}</code>}
        </div>
        <Button size="sm" variant="ghost" onClick={() => api.revealHistory(null).catch((e) => setError(String(e)))}>
          <FolderOpen className="h-4 w-4" />{t("about.showData")}
        </Button>
      </Panel>

      <section className="flex flex-col gap-3">
        <div className="flex items-center justify-between gap-3">
          <h2 className="flex items-center gap-2 font-display text-2xl font-semibold"><ScrollText className="h-5 w-5" aria-hidden />{t("about.changelog")}</h2>
          <Button size="sm" variant="ghost" onClick={() => open("changelog")}>{t("about.link.changelog")}<ExternalLink className="h-3 w-3 text-muted" aria-hidden /></Button>
        </div>
        {i18n.language !== "en" && <p className="text-sm text-muted">{t("about.changelogLang")}</p>}
        <Panel className="space-y-3 p-5 [&_code]:rounded [&_code]:bg-sunken [&_code]:px-1 [&_code]:text-[0.85em] [&_h2]:font-display [&_h2]:text-lg [&_h2]:font-semibold [&_h2:not(:first-child)]:pt-3 [&_li]:ml-4 [&_li]:list-disc [&_li]:text-ink/85 [&_ul]:space-y-1">
          {/* links in the file stay text: the webview would navigate away from the app */}
          <Markdown components={{ a: ({ children }) => <span className="underline decoration-line">{children}</span> }}>{changelogBody(changelog)}</Markdown>
        </Panel>
      </section>
    </div>
  );
}
