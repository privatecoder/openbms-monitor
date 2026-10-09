import i18n from "i18next";
import { initReactI18next } from "react-i18next";

const resources = {
  de: {
    translation: {
      app: "OpenBMS Monitor",
      nav: { dashboard: "Übersicht", help: "Hilfe" },
      conn: {
        title: "Verbindung", tcp: "Netzwerk", serial: "USB", host: "Gateway (Host:Port)",
        port: "Adapter wählen", bus: "Bus", busCan: "CAN-Buchse, 9600 Baud", busPack: "RS485-1/2, 19200 Baud",
        connect: "Verbinden", reconnect: "Neu verbinden", disconnect: "Trennen", scanning: "Suche Packs …",
        connected: "Verbunden", offline: "Nicht verbunden", found: "{{count}} Packs gefunden", none: "Keine Packs gefunden. Prüfe Bus, Baudrate und Verkabelung.",
        demo: "Vorschau mit aufgezeichneten Werten",
      },
      dash: {
        empty: "Wähle links Gateway oder Adapter und verbinde dich mit der Batterie.",
        head: { charging: "Lädt mit {{p}}", discharging: "Entlädt mit {{p}}", standby: "Im Leerlauf", off: "Ausgeschaltet", unknown: "Wartet auf Daten" },
        sub: "{{v}} und {{a}} über {{n}} Packs",
        tank: "{{remain}} von {{total}} Ah",
        chargeLimit: "Laden bis", dischargeLimit: "Entladen bis", at: "{{a}} bei {{v}}", blocked: "gesperrt",
        cellRange: "Zelldifferenz", cellRangeVal: "{{lo}} bis {{hi}}", tempRange: "Temperatur",
        rack: "Packs", pack: "Pack", master: "Master", strip: "Abweichung", soc: "Ladestand", voltage: "Spannung",
        current: "Strom", power: "Leistung", delta: "Spreizung", temp: "Temperatur", cycles: "Zyklen", capacity: "Kapazität",
        energy: "Energie", charged: "Geladen", dischargedNote: "{{v}} entladen", noAlarms: "Keine Meldungen", alarms: "Meldungen",
        alarmCount_one: "{{count}} Meldung", alarmCount_other: "{{count}} Meldungen",
        noData: "Keine Antwort von diesem Pack.", cells: "Zellspannungen", back: "Zur Übersicht", switches: "Schalter", port: "am Ausgang {{v}}",
        median: "Median aller Zellspannungen über alle Packs: {{v}}", stripRefPack: "Median der Zellspannungen dieses Packs: {{v}}", ambient: "Umgebung",
      },
      groups: {
        title: "Packs gruppieren", intro: "Fasse Packs zu Gruppen zusammen, zum Beispiel je Turm. Ein Pack gehört zu höchstens einer Gruppe; nicht zugeordnete Packs erscheinen unter „Ohne Gruppe“. Die Gruppen werden für diese Anlage gespeichert.",
        splitPre: "Aufteilen in", splitPost_one: "Gruppen, je {{count}} Pack", splitPost_other: "Gruppen, je {{count}} Packs", split: "Aufteilen", defaultName: "Turm", name: "Name der Gruppe",
        remove: "Gruppe entfernen", add: "Gruppe hinzufügen", save: "Gruppen speichern", cancel: "Abbrechen", clear: "Alle Gruppen entfernen",
        edit: "Gruppen bearbeiten", summary: "{{soc}}, {{a}}, {{p}}, Spreizung {{d}}", unassigned: "Ohne Gruppe",
      },
      live: { live: "Live, vor {{s}} s aktualisiert", stale: "Keine neuen Daten seit {{s}} s" },
      state: { charging: "Lädt", discharging: "Entlädt", standby: "Bereit", off: "Aus", unknown: "Unbekannt" },
      sw: { discharge: "Entladen", charge: "Laden", limiter: "Strombegrenzer", heater: "Heizung", on: "an", off: "aus" },
      sev: { info: "Hinweis", warning: "Warnung", protection: "Schutz", fault: "Fehler" },
      help: { more: "Ganze Erklärung öffnen", title: "Hilfe", missing: "Für diesen Punkt gibt es noch keinen Hilfetext.", related: "Siehe auch" },
    },
  },
  en: {
    translation: {
      app: "OpenBMS Monitor",
      nav: { dashboard: "Overview", help: "Help" },
      conn: {
        title: "Connection", tcp: "Network", serial: "USB", host: "Gateway (host:port)",
        port: "Select adapter", bus: "Bus", busCan: "CAN socket, 9600 baud", busPack: "RS485-1/2, 19200 baud",
        connect: "Connect", reconnect: "Reconnect", disconnect: "Disconnect", scanning: "Searching packs …",
        connected: "Connected", offline: "Not connected", found: "{{count}} packs found", none: "No packs found. Check bus, baud rate and wiring.",
        demo: "Preview with recorded values",
      },
      dash: {
        empty: "Choose a gateway or adapter on the left and connect to the battery.",
        head: { charging: "Charging at {{p}}", discharging: "Discharging at {{p}}", standby: "Idle", off: "Switched off", unknown: "Waiting for data" },
        sub: "{{v}} and {{a}} across {{n}} packs",
        tank: "{{remain}} of {{total}} Ah",
        chargeLimit: "Charge up to", dischargeLimit: "Discharge down to", at: "{{a}} at {{v}}", blocked: "blocked",
        cellRange: "Cell difference", cellRangeVal: "{{lo}} to {{hi}}", tempRange: "Temperature",
        rack: "Packs", pack: "Pack", master: "Master", strip: "Deviation", soc: "Charge", voltage: "Voltage",
        current: "Current", power: "Power", delta: "Spread", temp: "Temperature", cycles: "Cycles", capacity: "Capacity",
        energy: "Energy", charged: "Charged", dischargedNote: "{{v}} discharged", noAlarms: "No messages", alarms: "Messages",
        alarmCount_one: "{{count}} message", alarmCount_other: "{{count}} messages",
        noData: "No answer from this pack.", cells: "Cell voltages", back: "Back to overview", switches: "Switches", port: "{{v}} at the output",
        median: "Median of all cell voltages across all packs: {{v}}", stripRefPack: "Median of this pack's cell voltages: {{v}}", ambient: "Ambient",
      },
      groups: {
        title: "Group packs", intro: "Combine packs into groups, for example one per tower. A pack belongs to at most one group; unassigned packs appear under “Ungrouped”. Groups are saved for this installation.",
        splitPre: "Split into", splitPost_one: "groups of {{count}} pack", splitPost_other: "groups of {{count}} packs", split: "Split", defaultName: "Tower", name: "Group name",
        remove: "Remove group", add: "Add group", save: "Save groups", cancel: "Cancel", clear: "Remove all groups",
        edit: "Edit groups", summary: "{{soc}}, {{a}}, {{p}}, spread {{d}}", unassigned: "Ungrouped",
      },
      live: { live: "Live, updated {{s}} s ago", stale: "No new data for {{s}} s" },
      state: { charging: "Charging", discharging: "Discharging", standby: "Standby", off: "Off", unknown: "Unknown" },
      sw: { discharge: "Discharge", charge: "Charge", limiter: "Current limiter", heater: "Heater", on: "on", off: "off" },
      sev: { info: "Info", warning: "Warning", protection: "Protection", fault: "Fault" },
      help: { more: "Open full explanation", title: "Help", missing: "There is no help text for this item yet.", related: "See also" },
    },
  },
};

i18n.use(initReactI18next).init({
  resources,
  lng: (() => { try { return localStorage.getItem("lang"); } catch { return null; } })() ?? (navigator.language.toLowerCase().startsWith("de") ? "de" : "en"),
  fallbackLng: "en",
  interpolation: { escapeValue: false },
});
i18n.on("languageChanged", (l) => { try { localStorage.setItem("lang", l); } catch { /* ignore */ } });

export default i18n;
