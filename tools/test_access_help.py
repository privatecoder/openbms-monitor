"""Checks of the help generator's access facts: `python3 -m unittest discover -s tools` (writes nothing)."""
import copy
import re
import unittest

import access_help
import gen_help

# the topics whose bus and firmware statements come from the catalog
MIGRATED = ["topic.access", "topic.buses", "topic.params", "topic.paramWrite", "topic.bmsHistory", "topic.control",
            "topic.memoryDiagnosis", "topic.remoteDiagnosis"]
# the second round: maintenance, firmware, the release bits and calibration (their shorts: those that name no port of
# their own purpose; maintenance is about the RS485-1/2 bus, the profile about the CAN socket connection)
MIGRATED2 = ["topic.maintenance", "topic.firmware", "topic.firmwareProfile", "topic.patchGates", "topic.calibration"]
SHORT_FREE = ["topic.firmware", "topic.patchGates", "topic.calibration"]
GATED = ["switch.0.0", "switch.0.2", "switch.0.3", "switch.0.3p", "switch.0.4", "switch.7.5"]
PORT = re.compile(r"CAN-Buchse|CAN socket|RS485-1/2|UART[012]")


def page(tid, lang):
    _, de, en = gen_help.TOPICS[tid]
    title, short, body = de if lang == "de" else en
    return gen_help.fill(lang, short, where=tid), gen_help.fill(lang, body, True, tid)


class Placeholders(unittest.TestCase):
    def test_a_broken_placeholder_fails(self):
        for bad in ["{ACCESS:read-clock", "x {ACCESS:read-clock y", "{ACCESS:}", "{ACCESS:read-clock,}", "{ACCESS:no-such}",
                    "{access:read-clock}"]:
            for markdown in (False, True):
                with self.subTest(bad=bad, markdown=markdown), self.assertRaises(access_help.AccessError):
                    access_help.fill("de", bad, markdown)

    def test_a_body_block_stands_on_its_own_line(self):
        with self.assertRaises(access_help.AccessError):
            access_help.fill("de", "Wege: {ACCESS:read-clock}", True)
        self.assertTrue(access_help.fill("de", "Wege:\n{ACCESS:read-clock}", True).startswith("Wege:\n- **Uhr lesen**"))

    def test_a_broken_via_or_gate_fails(self):
        for bad in ["{VIA:", "{VIA:}", "{VIA:no-such}", "{via:flash}", "{GATE:0}", "{GATE:0.3", "{GATE:x.y}", "{gate:0.3}",
                    "{GATE:9.9}", "{GATE:0.5}"]:
            for markdown in (False, True):
                with self.subTest(bad=bad, markdown=markdown), self.assertRaises(access_help.AccessError):
                    access_help.fill("de", bad, markdown)

    def test_via_and_gate_read_as_part_of_a_sentence(self):
        self.assertEqual(access_help.fill("de", "geht {VIA:calibrate}", False), "geht über den RS485-1/2-Bus")
        self.assertEqual(access_help.fill("en", "works {VIA:maint-stop}", True), "works via the RS485-1/2 bus or the CAN socket")
        self.assertEqual(access_help.gate("de", "0.2"), "„Wartung starten“ über die CAN-Buchse")
        # the names stand quoted with their own casing, so an imperative English name still reads inside a sentence
        self.assertTrue(access_help.gate("en", "0.3").startswith("“Set clock”, “On/off”, "))
        self.assertIn("applies to “Write parameters and switches” and “Set the hard stop", switch("switch.0.0", "en"))

    def test_the_generator_refuses_what_is_left(self):
        for bad in ["{MIN", "{BUILDS} {NEW}", "{ACCESS:read-clock", "{VIA:flash", "{GATE:0.3"]:
            with self.subTest(bad=bad), self.assertRaises((SystemExit, access_help.AccessError)):
                gen_help.fill("en", bad, where="test")

    def test_the_probe_names_patched_firmware_not_ext_alone(self):
        # the supported first MAINT 1.6 (MNT format 3) answers the detection query too
        for lang in ("de", "en"):
            text = access_help.block(lang, ["read-identity"]) + access_help.plain(lang, ["read-identity"])
            self.assertRegex(text, "Patch-Firmware|patched firmware")
            self.assertNotRegex(text, "meldet nur EMU1101-EXT|only EMU1101-EXT")


class OneTruth(unittest.TestCase):
    """A way removed from the catalog leaves no claim behind: only the generated parts name it."""

    GONE = {"restart", "power-on-off", "read-calibration", "read-soc-store", "read-parameters", "write-parameters",
            "read-serials", "read-history", "set-clock", "read-diagnosis"}

    def setUp(self):
        self.catalog = access_help.CATALOG
        fewer = copy.deepcopy(self.catalog)
        fewer["routes"] = [r for r in fewer["routes"] if not (r["channel"] == "uart1" and r["action"] in self.GONE)]
        self.fewer = fewer

    def tearDown(self):
        access_help.CATALOG = self.catalog

    def test_the_short_texts_name_no_port_of_their_own(self):
        for tid in MIGRATED + SHORT_FREE:
            for i, lang in ((1, "de"), (2, "en")):
                short = gen_help.TOPICS[tid][i][1]
                self.assertNotRegex(access_help.PLACEHOLDER.sub("", short), PORT, f"{tid} {lang}")

    def test_removing_ways_removes_them_from_the_pages(self):
        before = {(t, l): page(t, l) for t in MIGRATED for l in ("de", "en")}
        access_help.CATALOG = self.fewer
        for (tid, lang), (short, body) in before.items():
            short2, body2 = page(tid, lang)
            for a in self.GONE:
                name = access_help._t(lang, "act." + a)
                can = access_help._t(lang, "channel.uart1")
                block = re.search(rf"- \*\*{re.escape(name)}\*\*\n((?:  - .*\n?)*)", body2)
                if block:
                    self.assertNotIn(can, block.group(1), f"{tid} {lang} {a}")
        # and the claims the old prose made by hand are gone
        for tid in MIGRATED:
            for lang in ("de", "en"):
                short, body = before[(tid, lang)]
                prose = "\n".join(l for l in body.split("\n") if not l.startswith("  - "))
                for claim in [r"nehmen \*\*Neu starten, Ein und Aus\*\* auch dort an", r"take \*\*restart, on and off\*\* there too",
                              r"auch über die CAN-Buchse, während der Master abfragt", r"also via the CAN socket while the master polls",
                              r"Dort zeigt das Pack auch seine Seriennummern", r"There the pack also shows its serial numbers"]:
                    self.assertNotRegex(short + prose, claim, f"{tid} {lang}")



def prose(text):
    """A source text without its generated parts."""
    for p in (access_help.PLACEHOLDER, access_help.VIA, access_help.GATE):
        text = p.sub("", text)
    return text


def switch(sid, lang):
    dt, ds, et, es = gen_help.SWITCHES[sid]
    return gen_help.fill(lang, ds if lang == "de" else es, where=sid)


class OneTruthMaintenance(unittest.TestCase):
    """Maintenance, firmware, release bits and calibration: a way or a bit need changed in the catalog changes the
    pages, and the prose left beside the generated parts makes no claim of its own about them."""

    def setUp(self):
        self.catalog = access_help.CATALOG

    def tearDown(self):
        access_help.CATALOG = self.catalog

    def with_routes(self, change):
        cat = copy.deepcopy(self.catalog)
        cat["routes"] = change(cat["routes"])
        access_help.CATALOG = cat

    def test_removing_a_way_removes_it_from_the_maintenance_and_firmware_pages(self):
        before = {(t, l): page(t, l)[1] for t in ("topic.maintenance", "topic.firmware") for l in ("de", "en")}
        self.with_routes(lambda rs: [r for r in rs if not (r["channel"] == "uart0" and r["action"] in ("maint-stop", "maint-status"))])
        for (tid, lang), body in before.items():
            body2 = page(tid, lang)[1]
            can = access_help._t(lang, "channel.uart0")
            for a in ("maint-stop", "maint-status"):
                name = re.escape(access_help._t(lang, "act." + a))
                old = re.search(rf"- \*\*{name}\*\*\n((?:  - .*\n?)*)", body).group(1)
                new = re.search(rf"- \*\*{name}\*\*\n((?:  - .*\n?)*)", body2).group(1)
                self.assertIn(can, old, f"{tid} {lang} {a}")
                self.assertNotIn(can, new, f"{tid} {lang} {a}")

    def test_a_new_port_shows_where_the_prose_names_one(self):
        def more(rs):
            extra = [dict(r, channel="uart1") for r in rs if r["action"] == "calibrate"]
            extra += [dict(r, channel="uart0") for r in rs if r["action"] == "maint-start"]
            return rs + extra
        self.with_routes(more)
        self.assertIn("Nur über den RS485-1/2-Bus oder die CAN-Buchse,", page("topic.calibration", "de")[0])
        self.assertIn("“Halt master” works via the RS485-1/2 bus or the CAN socket;", page("topic.maintenance", "en")[1])
        self.assertIn("„Wartung starten“ über den RS485-1/2-Bus oder die CAN-Buchse", page("topic.patchGates", "de")[1])

    def test_a_bit_need_removed_leaves_the_gate_lists(self):
        before = {l: (page("topic.patchGates", l)[1], switch("switch.0.3", l), switch("switch.0.0", l)) for l in ("de", "en")}
        def drop(rs):
            for r in rs:
                if r["channel"] == "uart1" and r["action"] in ("restart", "hard-stop"):
                    r["needs"] = [n for n in r["needs"] if n.get("kind") not in ("bit-set", "bit-clear") or n["group"] != 0]
            return rs
        self.with_routes(drop)
        for lang, (gates, s3, s0) in before.items():
            restart, hard = (access_help._t(lang, "act." + a) for a in ("restart", "hard-stop"))
            self.assertIn(restart, s3)
            self.assertIn(hard, s0)
            self.assertNotIn(restart, switch("switch.0.3", lang))
            self.assertNotIn(hard, switch("switch.0.0", lang))
            self.assertNotEqual(gates, page("topic.patchGates", lang)[1])
        # a bit that gates nothing any more fails, so no text keeps claiming the gate
        self.with_routes(lambda rs: [dict(r, needs=[n for n in r["needs"] if n.get("bit") != 2 or n.get("group") != 0]) for r in rs])
        with self.assertRaises(access_help.AccessError):
            page("topic.patchGates", "de")

    def test_on_off_and_clock_leave_bit_3_everywhere_on_the_page(self):
        # the short, the bit's own bullet and the update paragraph refer to the generated list, so taking the bit 3
        # need from on/off and set clock leaves no text that still ties them to bit 3
        words = {"de": re.compile(r"Ein/Aus|Uhr stellen|Uhr\b(?!-)"), "en": re.compile(r"[Oo]n/off|[Ss]et(ting)? (the )?clock|\bclock\b(?!”)")}
        def drop(rs):
            for r in rs:
                if r["action"] in ("power-on-off", "set-clock"):
                    r["needs"] = [n for n in r["needs"] if (n.get("group"), n.get("bit")) != (0, 3)]
            return rs
        before = {l: page("topic.patchGates", l) for l in ("de", "en")}
        self.with_routes(drop)
        for lang in ("de", "en"):
            short, body = page("topic.patchGates", lang)
            self.assertRegex(before[lang][1], words[lang])
            self.assertNotRegex(short, words[lang], lang)
            bit3 = re.search(r"- \*\*Bit 3[^\n]*", body).group(0)
            update = re.search(r"- \*\*(Beim Update|Updating)[^\n]*", body).group(0)
            for text in (bit3, update):
                self.assertNotRegex(text.replace("BMS-Uhr", "").replace("BMS clock", ""), words[lang], lang)
            for sid in ("switch.0.3", "switch.0.3p"):
                self.assertNotRegex(switch(sid, lang), words[lang], f"{sid} {lang}")

    def test_a_second_flash_port_leaves_no_claim_of_one(self):
        self.with_routes(lambda rs: rs + [dict(r, channel="uart1") for r in rs if r["action"] == "flash"])
        for lang in ("de", "en"):
            short, body = page("topic.firmware", lang)
            both = access_help._t(lang, "doc.via", ports=access_help._ports(lang, ["uart0", "uart1"]))
            self.assertIn(both, short)
            self.assertIn(access_help._t(lang, "channel.uart1"), re.search(r"- \*\*" + re.escape(access_help._t(lang, "act.flash")) + r"\*\*\n((?:  - .*\n?)*)", body).group(1))
            self.assertNotRegex(short + body, r"nur an einem Anschluss|on one port only|kein(en)? (anderen )?Update-Weg|no other update path", lang)

    def test_the_prose_keeps_no_claim_of_its_own(self):
        claims = [
            # where the marker, the maintenance start and the identity go
            r"über die CAN-Buchse am Master für EMU1101-EXT", r"via the CAN socket at the master for EMU1101-EXT",
            r"ab EMU1101-EXT an der eigenen Adresse jedes Packs, dort nur lesen", r"at every pack's own address, there read and clear",
            r"setzt den Marker über die CAN-Buchse", r"sets the marker over the CAN socket",
            r"Ab EMU1101-EXT \{MIN\} bietet die App Anhalten", r"From EMU1101-EXT \{MIN\} the app offers halt",
            r"Was der Master meldet \(CAN-Buchse\)", r"What the master reports \(CAN socket\)",
            r"Wartungszustand, Anhalten und Freigeben gibt es nur am Master", r"halt and release exist only at master",
            r"Neuere Builds beantworten die Kennung", r"newer builds answer the identity",
            # flashing and the identity before/after
            r"Über die CAN-Buchse geht kein Update", r"No update over the CAN socket", r"kein Update-Weg", r"no update port",
            r"nur EMU1101-EXT über RS485-1/2", r"only EMU1101-EXT via RS485-1/2",
            r"Auf dem RS485-1/2-Bus kennen sie erst EMU1101-EXT", r"only EMU1101-EXT knows it",
            r"Ab EMU1101-EXT liest „Platine flashen“", r"from EMU1101-EXT “Flash a board” reads",
            r"Ältere Patch-Versionen zeigen ihn nur über die CAN-Buchse", r"Older patch versions show it via the CAN socket only",
            # what the release bits gate
            r"Bit 0, Schreiben über die CAN-Buchse", r"Bit 0, writing via the CAN socket",
            r"1 erlaubt „Master anhalten“ über die CAN-Buchse", r"1 allows “Halt master” via the CAN socket",
            r"Uhr lesen, Parameter, Seriennummern und Versionsabfrage gehen immer", r"Reading the clock, parameters, serial numbers",
            r"Uhr lesen geht immer", r"reading the clock always works",
            r"bietet die App Neu starten, Ein/Aus \(Steuern\) und die BMS-Uhr über die CAN-Buchse", r"offers restart, on/off \(Control\) and the BMS clock via",
            r"gezieltes Aus an dieses Pack über den RS485-1/2-Bus oder die CAN-Buchse", r"directly via the RS485-1/2 bus or the CAN socket",
            r"gilt für gezieltes Aus über", r"applies to a direct off via",
            r"Bei EMU1101-EXT wird der Wartungsstart", r"With EMU1101-EXT the maintenance start",
            # calibration
            r"Nur über den RS485-1/2-Bus", r"Only via the RS485-1/2 bus",
            # bit 3 by hand beside its list, and the firmware's one port
            r"in Builds mit Ein/Aus und Uhr, deren Sperre", r"in builds with on/off and the clock, their lock",
            r"Bit 3 auf 1 \(etwa der Altwert FF\) Ein/Aus", r"allows on/off and setting the clock, with the first",
            r"Bit 3, Ein/Aus", r"Bit 3, on/off", r"mit Ein/Aus und Uhr im Build", r"with on/off and the clock in the build",
            r"hört nur an einem Anschluss", r"listens on one port only", r"no other update path", r"lassen sich aus dem BMS nicht auslesen", r"does not let the factors be read",
        ]
        texts = []
        for tid in MIGRATED2:
            _, de, en = gen_help.TOPICS[tid]
            texts += [(tid, prose(x)) for x in de + en]
        for sid in GATED:
            texts += [(sid, prose(x)) for x in gen_help.SWITCHES[sid]]
        import param_help
        texts += [("group.0", prose(x)) for x in param_help.GROUP_DETAILS["group.0"]]
        for where, text in texts:
            for claim in claims:
                self.assertNotRegex(text, claim, where)

    def test_the_bit_texts_name_no_port_beside_their_gate_list(self):
        # what a bit gates is the generated list; beside it only the way back (changing the bit) names a port
        for sid in GATED:
            for text in (prose(gen_help.SWITCHES[sid][1]), prose(gen_help.SWITCHES[sid][3])):
                self.assertNotRegex(text, r"CAN-Buchse|CAN socket", sid)
        for i in (1, 2):
            body = gen_help.TOPICS["topic.patchGates"][i][2]
            for head in re.findall(r"- \*\*(Bit \d[^*]*)\*\*", body):
                self.assertNotRegex(head, PORT, head)


if __name__ == "__main__":
    unittest.main()
