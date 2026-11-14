"""Access facts for the help pages, from the app's access catalog (`app/src/generated/access.json`, written by the Rust test
`the_ui_catalog_is_current`) and the shared names and templates (`app/src/i18n.access.json`, merged into `i18n.ts` as `acc`).

`{ACCESS:<action>[,<action>…]}` in a help text renders every way of each action: in a body as a list (port, firmware need,
other conditions, hints, whether the app uses it), in a title, short text or parameter file comment as one line of the
ways the app uses, short (no versions or bit numbers). Several ways of one action are alternatives; the conditions of one way all hold together; a read
capability mask needs at least one of its bits. This only describes the catalog; it decides nothing.

The short form must match `accessText` in `app/src/access.ts` (`access.test.ts` compares both on `generated/access_text.json`).
"""
import json
import os
import re

_HERE = os.path.dirname(__file__)
CATALOG = json.load(open(os.path.join(_HERE, "..", "app", "src", "generated", "access.json"), encoding="utf-8"))
TEXT = json.load(open(os.path.join(_HERE, "..", "app", "src", "i18n.access.json"), encoding="utf-8"))
ACTIONS = list(dict.fromkeys(r["action"] for r in CATALOG["routes"]))
PROBES = set(CATALOG["probe_actions"])
POLARITY = {tuple(b) for b in CATALOG["build_polarity"]}
PLACEHOLDER = re.compile(r"\{ACCESS:([^{}\n]*)\}")
# what is left of a placeholder after resolving (no closing brace, a line break or a brace inside, a wrong spelling)
LEFTOVER = re.compile(r"\{\s*ACCESS|\{\s*access\s*:", re.I)


class AccessError(ValueError):
    pass


def _get(lang, key):
    node = TEXT[lang]
    for part in key.split("."):
        if not isinstance(node, dict) or part not in node:
            raise AccessError(f"{lang}: no text acc.{key}")
        node = node[part]
    if not isinstance(node, str):
        raise AccessError(f"{lang}: acc.{key} is not a text")
    return node


def _t(lang, key, **vars):
    text = _get(lang, key)
    for k, v in vars.items():
        text = text.replace("{{" + k + "}}", str(v))
    if "{{" in text:
        raise AccessError(f"{lang}: acc.{key} left a variable: {text}")
    return text


def _ext_name(v):
    line = "Light " if v[:2] == [1, 5] else "Full " if v[:2] == [1, 6] else ""
    return line + ".".join(str(n) for n in v)


def _bits(mask):
    return [b for b in range(16) if mask & (1 << b)]


def _min_build(firmware):
    """`routeNeed` of access.ts: the first supported build that has every firmware need of the way."""
    lines, flags, caps = CATALOG["lines"], CATALOG["flag_since"], CATALOG["cap_since"]
    best = None
    for f in firmware:
        if f["kind"] == "patch-from":
            v = lines.get(".".join(map(str, f["version"])), f["version"] + [0])
        elif f["kind"] == "read-cap":
            v = next((caps[str(b)] for b in _bits(f["mask"]) if str(b) in caps), lines["1.5"])
        elif f["kind"] == "image-flag":
            v = flags.get(str(f["bit"]), lines["1.5"])
        else:
            raise AccessError(f"unknown firmware need {f}")
        if best is None or v > best:
            best = v
    return best


def _firmware(lang, firmware, detail):
    if not firmware:
        return None
    features = []
    for f in firmware:
        if f["kind"] == "image-flag":
            name = _t(lang, f"fw.flagName.b{f['bit']}")
            features.append(_t(lang, "doc.flag", name=name, bit=f["bit"]) if detail else name)
        elif f["kind"] == "read-cap":
            bits = _bits(f["mask"])
            names = _t(lang, "doc.or").join(_t(lang, f"fw.capName.c{b}") for b in bits)
            features.append(_t(lang, "doc.capBits", names=names, bits=_t(lang, "doc.or").join(map(str, bits))) if detail else names)
        elif f["kind"] != "patch-from":
            raise AccessError(f"unknown firmware need {f}")
    if not detail:
        # the hint form: no version, no bit numbers (the help and the matrix have them)
        return _t(lang, "doc.short.extWith", features=", ".join(features)) if features else _t(lang, "doc.short.ext")
    v = _ext_name(_min_build(firmware))
    if features:
        return _t(lang, "doc.extWith", v=v, features=_t(lang, "doc.and").join(features))
    return _t(lang, "doc.ext", v=v)


def _need(lang, n, detail):
    kind = n["kind"]
    if kind in ("bit-set", "bit-clear"):
        g, b = n["group"], n["bit"]
        name = _t(lang, f"bitName.g{g}b{b}")
        # the direction of these bits depends on the build: the switch texts give it, not a generated value
        if (g, b) in POLARITY:
            return _t(lang, "doc.bitBuild", g=g, b=b, name=name) if detail else _t(lang, "doc.short.release", name=name)
        if not detail:
            return _t(lang, "doc.short.on" if kind == "bit-set" else "doc.short.off", name=name)
        return _t(lang, "doc.bit", g=g, b=b, name=name, want=1 if kind == "bit-set" else 0)
    if kind == "master-only":
        return _t(lang, "doc.masterOnly" if detail else "doc.short.masterOnly")
    if kind == "marker":
        return _t(lang, "doc.marker", state=_t(lang, "markerState." + ("on" if n["on"] else "off")))
    if kind == "base-from":
        return _t(lang, "doc.base" if detail else "doc.short.base", v=".".join(f"{x:02d}" for x in n["base"]))
    if kind == "with-slaves":
        return _t(lang, "doc.withSlaves")
    if kind == "local-board":
        return _t(lang, "doc.localBoard")
    if kind == "quiet-bus":
        return None  # a hint, not a condition the app enforces
    raise AccessError(f"unknown need {n}")


def _conditions(lang, route, detail):
    """The conditions of one way, all of them needed; with `detail` also the hints and whether the app uses it."""
    conds = []
    if route["action"] in PROBES:
        conds.append(_t(lang, "probe" if detail else "doc.short.probe"))
    fw = _firmware(lang, route["firmware"], detail)
    if fw:
        conds.append(fw)
    elif route["action"] not in PROBES and not any(n["kind"] == "base-from" for n in route["needs"]):
        conds.append(_t(lang, "doc.oem" if detail else "doc.short.oem"))
    # the firmware base first, with the firmware
    needs = sorted(route["needs"], key=lambda n: n["kind"] != "base-from")
    conds += [c for c in (_need(lang, n, detail) for n in needs) if c]
    if route.get("yields"):
        conds.append(_t(lang, f"doc.{'yields' if detail else 'short.yields'}.{route['yields']}"))
    if detail:
        if any(n["kind"] == "quiet-bus" for n in route["needs"]):
            conds.append(_t(lang, "doc.hint", text=_t(lang, "doc.quietBus")))
        if not route["implemented"]:
            conds.append(_t(lang, "doc.notInApp"))
    return conds


def _routes(action):
    if action not in ACTIONS:
        raise AccessError(f"unknown action {action!r}")
    return [r for r in CATALOG["routes"] if r["action"] == action]


def _join_ports(lang, ways):
    """[(port, conditions)] → „A oder B (c)“: ports with the same conditions together, alternatives joined by „oder“."""
    groups = []
    for port, conds in ways:
        same = next((g for g in groups if g[0] == conds), None)
        if same:
            same[1].append(port)
        else:
            groups.append((conds, [port]))
    o = _t(lang, "doc.or")
    return o.join(f"{o.join(ports)} ({conds})" for conds, ports in groups)


def plain(lang, actions):
    """The hint form, one line. Several actions over the same ports share their common conditions („CAN-Buchse
    (EMU1101-EXT; Parameter schreiben: Schreibfreigabe an)“); otherwise one sentence per group of actions with the same
    ways. The action names only where the actions differ (the UI shows the action already)."""
    per = []  # per action: [(channel, [conditions])] of the ways the app uses
    for a in actions:
        per.append((a, [(r["channel"], _conditions(lang, r, False)) for r in _routes(a) if r["implemented"]]))
    port = lambda ch: _t(lang, "doc.port." + ch)
    if len(per) > 1 and all(w for _, w in per) and len({tuple(ch for ch, _ in w) for _, w in per}) == 1:
        merged = []
        for i, (ch, _) in enumerate(per[0][1]):
            common = [c for c in per[0][1][i][1] if all(c in w[i][1] for _, w in per)]
            if not common:
                break
            text = ", ".join(common)
            for a, w in per:
                extra = [c for c in w[i][1] if c not in common]
                if extra:
                    text += f"; {_t(lang, 'act.' + a)}: {', '.join(extra)}"
            merged.append((port(ch), text))
        else:
            return _join_ports(lang, merged) + "."
    groups = []
    for a, w in per:
        text = _join_ports(lang, [(port(ch), ", ".join(c)) for ch, c in w]) if w else _t(lang, "doc.notInApp")
        same = next((g for g in groups if g[1] == text), None)
        if same:
            same[0].append(a)
        else:
            groups.append(([a], text))
    if len(groups) == 1:
        return groups[0][1] + "."
    return " ".join(f"{', '.join(_t(lang, 'act.' + a) for a in names)}: {text}." for names, text in groups)


def block(lang, actions):
    """A Markdown list: every action under its name, below it every way (also the ones the app does not use)."""
    lines = []
    for a in actions:
        routes = _routes(a)
        lines.append(f"- **{_t(lang, 'act.' + a)}**")
        lines += [f"  - {_t(lang, 'channel.' + r['channel'])}: {'; '.join(_conditions(lang, r, True))}." for r in routes]
    return "\n".join(lines)


def _actions(spec):
    actions = [a.strip() for a in spec.split(",")]
    if not actions or any(not a for a in actions):
        raise AccessError(f"empty action in {{ACCESS:{spec}}}")
    return actions


def fill(lang, text, markdown):
    """Resolve `{ACCESS:…}`: a block in a Markdown body (on a line of its own), else the plain line."""
    if markdown:
        def sub_block(m):
            start = text.rfind("\n", 0, m.start()) + 1
            end = text.find("\n", m.end())
            if text[start:m.start()].strip() or text[m.end():end if end >= 0 else len(text)].strip():
                raise AccessError(f"{m.group(0)} must stand on a line of its own in a body")
            return block(lang, _actions(m.group(1)))
        out = PLACEHOLDER.sub(sub_block, text)
    else:
        out = PLACEHOLDER.sub(lambda m: plain(lang, _actions(m.group(1))), text)
    left = LEFTOVER.search(out)
    if left:
        raise AccessError(f"unresolved placeholder near {out[left.start():left.start() + 40]!r}")
    return out


def plain_all():
    """The plain line of every action in both languages, for the parity test with `accessText` in access.ts."""
    return {lang: {a: plain(lang, [a]) for a in ACTIONS} for lang in ("de", "en")}


def min_patch():
    """`{MIN}`: the lowest supported builds of both patch lines, from the catalog's support lines."""
    return f"{_ext_name(CATALOG['lines']['1.5'])} / {_ext_name(CATALOG['lines']['1.6'])}"
