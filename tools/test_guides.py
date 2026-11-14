"""Checks of the step-by-step guides (`guides.py`) and their pages: `python3 -m unittest discover -s tools` (writes nothing)."""
import copy
import os
import re
import unittest

import gen_help
import guides

LINK = re.compile(r"\]\(#([^)]+)\)")
# a list line: its indent and marker ("1." or "-"), the structure both languages must share
ITEM = re.compile(r"^( *)(\d+\.|-) ", re.M)


def body(gid, lang):
    return gen_help.guide_body(lang, gid, guides.GUIDES[gid], gen_help.titles(lang))


def read(path):
    with open(path, encoding="utf-8") as f:
        return f.read()


def shape(steps):
    return [(len(s["cases"]), [shape(c["steps"]) for c in s["cases"]]) for s in steps]


class Guides(unittest.TestCase):
    def test_both_languages_have_text_everywhere(self):
        for gid, g in guides.GUIDES.items():
            for lang in ("de", "en"):
                title, short, _ = g[lang]
                self.assertTrue(title and short, f"{gid} {lang}")
            for s in gen_help.guide_steps(g["steps"]):
                self.assertTrue(s["de"].strip() and s["en"].strip(), gid)
                for c in s["cases"]:
                    self.assertTrue(c["de"].strip() and c["en"].strip() and c["steps"], gid)

    def test_german_and_english_share_steps_cases_and_targets(self):
        for gid, g in guides.GUIDES.items():
            de, en = body(gid, "de"), body(gid, "en")
            self.assertEqual(ITEM.findall(de), ITEM.findall(en), gid)
            self.assertEqual(LINK.findall(de), LINK.findall(en), gid)
            self.assertGreater(len(ITEM.findall(de)), 3, gid)
            self.assertTrue(shape(g["steps"]))

    def test_cases_are_alternatives_with_their_own_numbering(self):
        # a case list is a bullet list under a numbered step, each case numbering its steps from 1
        text = body("guide.firmware", "en")
        self.assertRegex(text, r"\n\d+\. .*\n {3,}- \*\*A – .*\*\*\n {5,}1\. ")
        for label in ("A – ", "B – ", "C – ", "C′ – "):
            self.assertIn(f"- **{label}", text)
        # C′ releases the master before flashing it
        cprime = text[text.index("- **C′"):]
        self.assertLess(cprime.index("Release master"), cprime.index("Flash the master"))

    def test_every_target_and_related_id_exists(self):
        known = gen_help.known_ids()
        gen_help.check_ids(known)
        for gid, g in guides.GUIDES.items():
            for target in LINK.findall(body(gid, "de")):
                self.assertIn(target, known, gid)
            self.assertTrue(set(g["related"]) <= set(known), gid)

    def test_an_unknown_target_or_related_id_fails(self):
        saved = copy.deepcopy(guides.GUIDES)
        try:
            guides.GUIDES["guide.setup"]["steps"][0]["help"] = "topic.noSuch"
            with self.assertRaises(SystemExit):
                gen_help.check_ids(gen_help.known_ids())
            guides.GUIDES.clear(); guides.GUIDES.update(copy.deepcopy(saved))
            guides.GUIDES["guide.setup"]["related"].append("topic.noSuch")
            with self.assertRaises(SystemExit):
                gen_help.check_ids(gen_help.known_ids())
        finally:
            guides.GUIDES.clear(); guides.GUIDES.update(saved)

    def test_an_id_in_two_tables_fails(self):
        gen_help.GUIDES["topic.firmware"] = guides.GUIDES["guide.setup"]
        try:
            with self.assertRaises(SystemExit):
                gen_help.known_ids()
        finally:
            del gen_help.GUIDES["topic.firmware"]

    def test_placeholders_resolve_and_a_broken_one_fails(self):
        for gid, g in guides.GUIDES.items():
            for lang in ("de", "en"):
                text = body(gid, lang) + gen_help.fill(lang, g[lang][0]) + gen_help.fill(lang, g[lang][1])
                self.assertNotRegex(text, r"\{[A-Z]")
        self.assertIn("EMU1101-EXT", body("guide.firmware", "de"))
        saved = guides.GUIDES["guide.setup"]["steps"][0]["de"]
        guides.GUIDES["guide.setup"]["steps"][0]["de"] = "geht {VIA:no-such}"
        try:
            with self.assertRaises(Exception):
                body("guide.setup", "de")
        finally:
            guides.GUIDES["guide.setup"]["steps"][0]["de"] = saved

    def test_the_pages_are_current_and_the_topics_link_back(self):
        root = os.path.join(os.path.dirname(__file__), "..", "help")
        for gid, g in guides.GUIDES.items():
            for lang in ("de", "en"):
                page = read(os.path.join(root, lang, f"{gid}.md"))
                self.assertTrue(page.endswith(body(gid, lang) + "\n"), f"{gid} {lang}: run tools/gen_help.py")
            for tid in g["topics"]:
                for lang in ("de", "en"):
                    related = re.search(r"^related: (.*)$", read(os.path.join(root, lang, f"{tid}.md")), re.M).group(1)
                    own = [r.strip() for r in gen_help.TOPICS[tid][0].split(",") if r.strip()]
                    # appended, the topic's own related links kept in front
                    self.assertEqual(related.split(", ")[:len(own)], own, tid)
                    self.assertIn(gid, related.split(", "), tid)


if __name__ == "__main__":
    unittest.main()
