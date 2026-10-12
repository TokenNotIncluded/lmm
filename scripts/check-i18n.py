#!/usr/bin/env python3
"""Validate bundled UI catalogs and their Rust message references (stdlib only)."""
from __future__ import annotations

import json
import re
import sys
from collections import Counter
from pathlib import Path
from string import Formatter

CJK = re.compile(r"[\u3400-\u9fff\uf900-\ufaff]")
KEY = re.compile(r"[a-z][a-z0-9_]*(?:\.[a-z][a-z0-9_]*)+\Z")
REFERENCE = re.compile(r'\b(?:tr|trf!)\(\s*"([a-z0-9_.]+)"')
FIELD = re.compile(r"[a-zA-Z][a-zA-Z0-9_-]*\Z")


def unique_object(pairs: list[tuple[str, object]]) -> dict[str, object]:
    result: dict[str, object] = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate key: {key}")
        result[key] = value
    return result


def load_catalog(path: Path) -> dict[str, str]:
    value = json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=unique_object)
    if not isinstance(value, dict) or not value:
        raise ValueError("catalog must be a nonempty JSON object")
    for key, message in value.items():
        if not KEY.fullmatch(key):
            raise ValueError(f"invalid message key: {key!r}")
        if not isinstance(message, str) or not message.strip():
            raise ValueError(f"{key}: message must be a nonempty string")
        if any(ord(c) < 32 and c not in "\n\t" for c in message):
            raise ValueError(f"{key}: disallowed control character")
    return value


def placeholders(message: str) -> Counter[str]:
    result: Counter[str] = Counter()
    for _, name, spec, conversion in Formatter().parse(message):
        if name is not None:
            if not FIELD.fullmatch(name) or spec or conversion:
                raise ValueError(f"unsupported placeholder: {name!r}")
            result[name] += 1
    return result


def validate(root: Path) -> list[str]:
    errors: list[str] = []
    catalogs: dict[str, dict[str, str]] = {}
    for path in sorted((root / "locales").glob("*.json")):
        try:
            catalogs[path.stem] = load_catalog(path)
        except (ValueError, OSError) as error:
            errors.append(f"{path.name}: {error}")
    if "en" not in catalogs or "zh-CN" not in catalogs:
        errors.append("both en.json and zh-CN.json are required")
    english = catalogs.get("en", {})
    for name, messages in catalogs.items():
        missing = sorted(english.keys() - messages.keys())
        extra = sorted(messages.keys() - english.keys())
        if missing or extra:
            errors.append(f"{name}: missing keys {missing}; extra keys {extra}")
        for key, message in messages.items():
            try:
                fields = placeholders(message)
                if key in english and fields != placeholders(english[key]):
                    errors.append(f"{name}/{key}: placeholder mismatch")
            except ValueError as error:
                errors.append(f"{name}/{key}: {error}")
    if any(CJK.search(message) for message in english.values()):
        errors.append("en: unexpected CJK text in the English catalog")
    sources = sorted((root / "src").rglob("*.rs"))
    if not sources:
        errors.append("no Rust source files found")
    for path in sources:
        text = path.read_text(encoding="utf-8")
        production = "" if path.name == "tests.rs" else re.split(
            r"#\[cfg\(test\)\]\s*mod\s+\w+\s*\{", text, maxsplit=1
        )[0]
        for line, content in enumerate(production.splitlines(), 1):
            if CJK.search(content):
                errors.append(f"{path.relative_to(root)}:{line}: move CJK UI text to locales")
        references = set(REFERENCE.findall(production))
        references.update(re.findall(r'\btext\([^,\n]+,\s*"([a-z0-9_.]+)"', production))
        if path.name == "catalog.rs":
            references.update(re.findall(r'"(software\.[a-z0-9_]+)"', text))
        for key in sorted(references - english.keys()):
            errors.append(f"{path.relative_to(root)}: missing message {key}")
    return errors


def main() -> int:
    root = Path(__file__).resolve().parents[1]
    errors = validate(root)
    if errors:
        print("\n".join(errors), file=sys.stderr)
        return 1
    count = len(load_catalog(root / "locales/en.json"))
    print(f"i18n checks passed: {count} keys per catalog; source references and placeholders match.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
