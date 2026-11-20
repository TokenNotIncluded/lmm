#!/usr/bin/env python3
"""Validate embedded translations and Rust references with the standard library."""
from __future__ import annotations

import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
HAN = re.compile("[\u3400-\u4dbf\u4e00-\u9fff\uf900-\ufaff\U00020000-\U0003134f]")
KEY = re.compile(r"[a-z][a-z0-9_]*(?:\.[a-z][a-z0-9_]*)+\Z")
PLACEHOLDER = re.compile(r"\{([a-zA-Z][a-zA-Z0-9_-]*)\}")
CALL_KEY = re.compile(r'(?<![\w.])(?:msg|text|english)\(\s*"([^"\n]+)"')
MATCH_KEY = re.compile(r'Self::\w+\s*=>\s*"([a-z]+\.[a-z_]+)"')


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
            raise ValueError(f"{key}: expected a nonempty string")
        if any(ord(char) < 32 and char not in "\n\t" for char in message):
            raise ValueError(f"{key}: disallowed control character")
    return value


def placeholders(value: str) -> list[str]:
    remainder = PLACEHOLDER.sub("", value)
    if "{" in remainder or "}" in remainder:
        raise ValueError(f"invalid placeholder syntax: {value!r}")
    return sorted(PLACEHOLDER.findall(value))


def validate(root: Path) -> list[str]:
    errors: list[str] = []
    catalogs: dict[str, dict[str, str]] = {}
    for path in sorted((root / "locales").glob("*.json")):
        try:
            catalogs[path.stem] = load_catalog(path)
        except (ValueError, OSError) as error:
            errors.append(f"{path.name}: {error}")
    if not {"en", "zh-CN"}.issubset(catalogs):
        errors.append("both en.json and zh-CN.json are required")
    reference = catalogs.get("en", {})
    for locale, catalog in catalogs.items():
        missing = sorted(reference.keys() - catalog.keys())
        extra = sorted(catalog.keys() - reference.keys())
        if missing or extra:
            errors.append(f"{locale}: missing keys {missing}; extra keys {extra}")
        for key, value in catalog.items():
            try:
                fields = placeholders(value)
                if key in reference and fields != placeholders(reference[key]):
                    errors.append(f"{locale}.{key}: placeholders differ from English")
            except ValueError as error:
                errors.append(f"{locale}.{key}: {error}")
    if any(HAN.search(value) for value in reference.values()):
        errors.append("en: unexpected CJK text in the English catalog")
    used: set[str] = set()
    paths = sorted((root / "src").rglob("*.rs"))
    if not paths:
        errors.append("no Rust source files found")
    for path in paths:
        source = path.read_text(encoding="utf-8")
        if HAN.search(source):
            errors.append(f"{path.relative_to(root)}: move CJK UI text to locales/")
        used.update(CALL_KEY.findall(source))
        used.update(MATCH_KEY.findall(source))
    for key in sorted(used - reference.keys()):
        errors.append(f"missing message {key}")
    unused = sorted(reference.keys() - used)
    if unused:
        errors.append(f"unused message keys: {unused}")
    return errors


def main() -> int:
    errors = validate(ROOT)
    if errors:
        print("\n".join(errors), file=sys.stderr)
        return 1
    count = len(load_catalog(ROOT / "locales/en.json"))
    print(f"i18n: 2 catalogs, {count} keys; values, placeholders and runtime references verified")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
