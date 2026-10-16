#!/usr/bin/env python3
"""Check embedded translations with only the Python standard library."""
from __future__ import annotations

import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
HAN = re.compile("[\u3400-\u4dbf\u4e00-\u9fff\uf900-\ufaff\U00020000-\U0003134f]")
PLACEHOLDER = re.compile(r"\{([a-zA-Z][a-zA-Z0-9_-]*)\}")
CALL_KEY = re.compile(r'(?<![\w.])(?:msg|text)\("([^"\n]+)"\)')
MATCH_KEY = re.compile(r'Self::\w+\s*=>\s*"([a-z]+\.[a-z_]+)"')


def unique_object(pairs: list[tuple[str, object]]) -> dict[str, object]:
    result: dict[str, object] = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate translation key: {key}")
        result[key] = value
    return result


def placeholders(value: str) -> list[str]:
    remainder = PLACEHOLDER.sub("", value)
    if "{" in remainder or "}" in remainder:
        raise ValueError(f"invalid placeholder syntax: {value!r}")
    return sorted(PLACEHOLDER.findall(value))


def main() -> None:
    catalogs = {
        path.stem: json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=unique_object)
        for path in sorted((ROOT / "locales").glob("*.json"))
    }
    reference = catalogs["en"]
    for locale, catalog in catalogs.items():
        if set(catalog) != set(reference):
            raise ValueError(f"{locale}: missing={set(reference) - set(catalog)}, extra={set(catalog) - set(reference)}")
        for key, value in catalog.items():
            if not isinstance(value, str) or not value.strip():
                raise ValueError(f"{locale}.{key}: expected a nonempty string")
            if placeholders(value) != placeholders(reference[key]):
                raise ValueError(f"{locale}.{key}: placeholders differ from English")
    used: set[str] = set()
    for path in (ROOT / "src").rglob("*.rs"):
        source = path.read_text(encoding="utf-8")
        if HAN.search(source):
            raise ValueError(f"{path.relative_to(ROOT)}: move Chinese UI text into locales/")
        used.update(CALL_KEY.findall(source))
        used.update(MATCH_KEY.findall(source))
    unknown = used - set(reference)
    unused = set(reference) - used
    if unknown or unused:
        raise ValueError(f"unknown message keys={unknown}; unused message keys={unused}")
    print(f"i18n: {len(catalogs)} catalogs, {len(reference)} keys; placeholders and runtime references verified")


if __name__ == "__main__":
    main()
