"""Checks of the help generator's access facts: `python3 -m unittest discover -s tools` (writes nothing)."""
import copy
import re
import unittest

import access_help
import gen_help

# the topics whose bus and firmware statements come from the catalog
MIGRATED = ["topic.access", "topic.buses", "topic.params", "topic.paramWrite", "topic.bmsHistory", "topic.control",
            "topic.memoryDiagnosis", "topic.remoteDiagnosis"]
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

    def test_the_generator_refuses_what_is_left(self):
        for bad in ["{MIN", "{BUILDS} {NEW}", "{ACCESS:read-clock"]:
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
        for tid in MIGRATED:
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


if __name__ == "__main__":
    unittest.main()
