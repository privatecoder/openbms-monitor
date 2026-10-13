import { useState } from "react";
import { useTranslation } from "react-i18next";
import type { Endpoint } from "../api";
import { Button } from "../components/ui/button";

const input = "h-9 w-full rounded-md border border-line bg-surface px-3 text-sm text-ink outline-none focus:border-charge";

/** Before a sidebar connection creates an installation: its name (default „Anlage n“). Cancel creates nothing. */
export function NameDialog({ endpoint, suggested, onConfirm, onCancel }: {
  endpoint: Endpoint;
  suggested: string;
  /** creates the installation; returns the refusal to show (the dialog stays), null when done */
  onConfirm: (name: string) => Promise<{ key: string; params?: Record<string, unknown> } | null>;
  onCancel: () => void;
}) {
  const { t } = useTranslation();
  const [name, setName] = useState(suggested);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const confirm = async () => {
    setSaving(true); setError(null);
    try { const r = await onConfirm(name); if (r) setError(t(r.key, r.params)); }
    finally { setSaving(false); }
  };
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-ink/20 p-4">
      <form role="dialog" aria-modal="true" aria-labelledby="name-dialog-title" className="w-full max-w-md space-y-3 rounded-md border border-line bg-surface p-5 text-sm"
        onSubmit={(e) => { e.preventDefault(); if (!saving) void confirm(); }}>
        <h2 id="name-dialog-title" className="font-display text-lg font-semibold">{t("maint.nameDialog.title")}</h2>
        <p>{t("maint.nameDialog.text", { endpoint: endpoint.kind === "tcp" ? endpoint.addr : endpoint.path })}</p>
        <label className="block space-y-1">{t("maint.setup.name")}
          <input className={input} value={name} autoFocus onChange={(e) => setName(e.target.value)} />
        </label>
        {error && <p className="text-alarm">{error}</p>}
        <p className="text-muted">{t("maint.nameDialog.hint")}</p>
        <div className="flex justify-end gap-2">
          <Button type="button" variant="secondary" size="sm" disabled={saving} onClick={onCancel}>{t("maint.nameDialog.cancel")}</Button>
          <Button type="submit" variant="primary" size="sm" disabled={saving}>{t("maint.nameDialog.create")}</Button>
        </div>
      </form>
    </div>
  );
}
