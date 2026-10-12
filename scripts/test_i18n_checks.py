#!/usr/bin/env python3
"""Regression tests for the catalog validator; no Rust toolchain is required."""
from __future__ import annotations

import importlib.util
import json
import tempfile
import unittest
from pathlib import Path

SPEC = importlib.util.spec_from_file_location("check_i18n", Path(__file__).with_name("check-i18n.py"))
assert SPEC is not None and SPEC.loader is not None
CHECK = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CHECK)


class CatalogChecks(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        (self.root / "src").mkdir()
        (self.root / "locales").mkdir()
        (self.root / "src/main.rs").write_text('fn main() { tr("test.message"); }\n', encoding="utf-8")
        self.write("en", {"test.message": "Hello {name}"})
        self.write("zh-CN", {"test.message": "你好，{name}"})

    def write(self, language: str, value: object) -> None:
        (self.root / f"locales/{language}.json").write_text(json.dumps(value, ensure_ascii=False), encoding="utf-8")

    def test_valid_catalogs(self) -> None:
        self.assertEqual(CHECK.validate(self.root), [])

    def test_duplicate_keys_are_rejected(self) -> None:
        (self.root / "locales/zh-CN.json").write_text('{"test.message":"a","test.message":"b"}', encoding="utf-8")
        self.assertTrue(any("duplicate key" in error for error in CHECK.validate(self.root)))

    def test_missing_and_extra_keys_are_rejected(self) -> None:
        self.write("zh-CN", {"test.other": "Other"})
        self.assertTrue(any("missing keys" in error and "extra keys" in error for error in CHECK.validate(self.root)))

    def test_placeholder_mismatch_and_malformed_template_are_rejected(self) -> None:
        for message in ["Hello {other}", "Hello {name", "Hello {name!r}"]:
            with self.subTest(message=message):
                self.write("zh-CN", {"test.message": message})
                self.assertTrue(CHECK.validate(self.root))

    def test_empty_and_non_string_messages_are_rejected(self) -> None:
        for message in ["", "  ", None, 1, {"nested": "value"}]:
            with self.subTest(message=message):
                self.write("zh-CN", {"test.message": message})
                self.assertTrue(CHECK.validate(self.root))

    def test_source_literals_and_missing_references_are_rejected(self) -> None:
        (self.root / "src/main.rs").write_text('fn main() { tr("test.missing"); println!("中文"); }', encoding="utf-8")
        errors = CHECK.validate(self.root)
        self.assertTrue(any("CJK UI text" in error for error in errors))
        self.assertTrue(any("missing message" in error for error in errors))

    def test_english_catalog_and_control_characters_are_checked(self) -> None:
        for message in ["中文 {name}", "Hello\u001b {name}"]:
            with self.subTest(message=message):
                self.write("en", {"test.message": message})
                self.assertTrue(CHECK.validate(self.root))


if __name__ == "__main__":
    unittest.main()
