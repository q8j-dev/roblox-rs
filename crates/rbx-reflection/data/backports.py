import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent

PRIMITIVES = {"bool", "int", "int64", "float", "double", "string"}
DATATYPE_ALIASES = {"ContentId": "Content", "Content": "Content", "OptionalCoordinateFrame": "OptionalCoordinateFrame"}


def type_name(t):
    if t is None:
        return "void"
    if isinstance(t, list):
        return "Tuple"
    cat, name = t.get("Category"), t.get("Name")
    if cat == "Primitive":
        return name if name in PRIMITIVES else "void" if name == "null" else name
    if cat == "Enum":
        return f"Enum:{name}"
    if cat == "Class":
        return f"Instance:{name}"
    if cat == "DataType":
        return DATATYPE_ALIASES.get(name, name)
    return name


def security(s, which="Read"):
    if isinstance(s, dict):
        s = s.get(which, "None")
    s = s or "None"
    return {
        "None": None,
        "PluginSecurity": "Plugin",
        "LocalUserSecurity": "LocalUser",
        "RobloxScriptSecurity": "RobloxScript",
        "RobloxSecurity": "Roblox",
        "RobloxEngineSecurity": "Roblox",
        "NotAccessibleSecurity": "NotAccessible",
    }.get(s, "Roblox")


def tags(x):
    return [t for t in (x.get("Tags") or []) if isinstance(t, str)]


def params(ps):
    return [{"name": p["Name"], "type": type_name(p.get("Type")), "default": p.get("Default")} for p in ps or []]


def convert_member(m, evidence):
    t = tags(m)
    deprecated = "" if "Deprecated" in t else None
    kind = m["MemberType"]
    if kind == "Property":
        flags = 1 | 2 | 4 | 8 | 16
        ser = m.get("Serialization") or {}
        if not ser.get("CanLoad", True):
            flags &= ~4
        if not ser.get("CanSave", True):
            flags &= ~8
        if "NotReplicated" in t:
            flags &= ~2
        if "NotScriptable" in t:
            flags &= ~16
        if "Hidden" in t:
            flags &= ~1
        return "properties", {
            "name": m["Name"],
            "type": type_name(m.get("ValueType")),
            "category": m.get("Category", "Data"),
            "flags": flags,
            "security": security(m.get("Security")),
            "read_only": "ReadOnly" in t,
            "deprecated_by": deprecated,
            "getter": None,
            "evidence": evidence,
        }
    if kind in ("Function", "Callback"):
        bucket = "callbacks" if kind == "Callback" else "yield_functions" if "Yields" in t else "functions"
        return bucket, {
            "name": m["Name"],
            "return": type_name(m.get("ReturnType")),
            "params": params(m.get("Parameters")),
            "security": security(m.get("Security")),
            "deprecated_by": deprecated,
            "evidence": evidence,
        }
    if kind == "Event":
        return "events", {
            "name": m["Name"],
            "params": params(m.get("Parameters")),
            "security": security(m.get("Security")),
            "deprecated_by": deprecated,
            "kind": "RemoteEventDesc" if "NotReplicated" not in t else "EventDesc",
            "evidence": evidence,
        }
    raise ValueError(kind)


def empty_class(c, evidence):
    t = tags(c)
    return {
        "name": c["Name"],
        "superclass": c["Superclass"] if c["Superclass"] != "<<<ROOT>>>" else None,
        "creatable": "NotCreatable" not in t,
        "service": "Service" in t,
        "flags": ["PERSISTENT_LOCAL"] if "NotReplicated" in t else [],
        "security": None,
        "properties": [],
        "functions": [],
        "yield_functions": [],
        "callbacks": [],
        "events": [],
        "evidence": evidence,
    }


def main():
    dump_path = Path(sys.argv[1])
    dump = json.loads(dump_path.read_text(encoding="utf-8"))
    classes = {c["Name"]: c for c in dump["Classes"]}
    enums = {e["Name"]: e for e in dump["Enums"]}
    old = {c["name"] for c in json.loads((HERE / "api.json").read_text(encoding="utf-8"))["classes"]}
    out_classes = {}
    out_enums = {}

    def add_class(name, evidence):
        c = classes[name]
        entry = out_classes.setdefault(name, empty_class(c, evidence))
        if entry.get("extends"):
            entry.pop("extends")
        entry["evidence"] = evidence
        for m in c["Members"]:
            if "Deprecated" in tags(m):
                continue
            bucket, rec = convert_member(m, evidence)
            if not any(x["name"] == rec["name"] for x in entry[bucket]):
                entry[bucket].append(rec)
        parent = c["Superclass"]
        if parent not in old and parent in classes and parent not in out_classes:
            add_class(parent, f"superclass of {name}")
    for raw in (HERE / "backports.txt").read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        selector, _, evidence = line.partition("|")
        kind, _, target = selector.strip().partition(" ")
        target = target.strip()
        evidence = evidence.strip()
        if kind == "class":
            add_class(target, evidence)
        elif kind == "member":
            cls, _, name = target.partition(".")
            c = classes[cls]
            owner = c
            while not any(m["Name"] == name for m in owner["Members"]):
                owner = classes[owner["Superclass"]]
            m = next(m for m in owner["Members"] if m["Name"] == name)
            entry = out_classes.setdefault(cls, {**empty_class(c, None), "extends": True})
            bucket, rec = convert_member(m, evidence)
            entry[bucket].append(rec)
        elif kind == "enum":
            e = enums[target]
            out_enums[target] = {
                "name": target,
                "items": [{"name": i["Name"], "value": i["Value"]} for i in e["Items"]],
                "legacy": [
                    {"name": legacy, "value": i["Value"]}
                    for i in e["Items"]
                    for legacy in (i.get("LegacyNames") or [])
                ],
                "evidence": evidence,
            }
        else:
            raise ValueError(line)
    result = {
        "source": dump_path.name,
        "classes": list(out_classes.values()),
        "enums": list(out_enums.values()),
    }
    (HERE / "backports.json").write_text(json.dumps(result, indent=1, sort_keys=True) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
