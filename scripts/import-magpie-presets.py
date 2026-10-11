"""Extract the pinned Magpie provider data without executing Go or using a network.

Only the syntax and helpers at the recorded revision are supported. Source and
output hashes make an upstream update an explicit review rather than a silent
catalog change. This script is a maintainer tool, not a CLI runtime dependency.
"""

import argparse
import hashlib
import json
import re
from pathlib import Path

REVISION = "d5cae267a727e23f4b6e2b28aa107ecdeac5aa9c"
SOURCE_SHA256 = "3f3117872ad5b416e27ec9328962186195315d721e137b3a62f7c1143e17e0a2"
JSON_SHA256 = "4a3f5e683db4e6ed921fdb4b1762830095df67c5b29c69976f38d5fe84e70b4b"
TOKEN_PATTERN = re.compile(
    r'\s+|//[^\n]*|"(?:[^"\\]|\\.)*"|[A-Za-z_][A-Za-z_0-9]*|[][{}(),:]'
)
CONSTANTS = {
    "KindVendor": "vendor",
    "KindRelay": "relay",
    "KindLocal": "local",
    "KindRemote": "remote",
    "VertexPreset": "google-vertex",
    "AzurePreset": "azure",
    "RemoteMagpiePreset": "remote-magpie",
    "true": True,
    "false": False,
}


def tokenize(text):
    tokens = []
    position = 0
    for match in TOKEN_PATTERN.finditer(text):
        if text[position:match.start()].strip():
            raise ValueError("unsupported Go syntax")
        token = match.group()
        position = match.end()
        if not token.isspace() and not token.startswith("//"):
            tokens.append(token)
    if text[position:].strip():
        raise ValueError("unsupported trailing Go syntax")
    return tokens


def helper(name, args):
    if name == "bedrockChat":
        return f"https://bedrock-runtime.{args[0]}.amazonaws.com/openai/v1"
    if name == "bedrockAnthropic":
        return f"https://bedrock-runtime.{args[0]}.amazonaws.com/anthropic"
    if name == "bailianWorkspace":
        return f"https://{{WorkspaceId}}.{args[0]}.maas.aliyuncs.com/compatible-mode/v1"
    if name == "bedrockRegions":
        return [
            {
                "ID": region,
                "Name": region,
                "Chat": helper("bedrockChat", [region]),
                "Responses": helper("bedrockChat", [region]),
                "Anthropic": helper("bedrockAnthropic", [region]),
            }
            for region in args
        ]
    raise ValueError(f"unsupported upstream helper: {name}")


class LiteralParser:
    def __init__(self, text, constants):
        self.tokens = tokenize(text)
        self.position = 0
        self.constants = constants

    def take(self, expected=None):
        token = self.tokens[self.position]
        self.position += 1
        if expected is not None and token != expected:
            raise ValueError(f"expected {expected}, found {token}")
        return token

    def peek(self):
        if self.position < len(self.tokens):
            return self.tokens[self.position]
        return None

    def sequence(self, end):
        values = []
        while self.peek() != end:
            values.append(self.value())
            if self.peek() != end:
                self.take(",")
        self.take(end)
        return values

    def value(self):
        token = self.take()
        if token.startswith('"'):
            return json.loads(token)
        if token == "[":
            self.take("]")
            if self.take() not in ("string", "Region", "PresetDef"):
                raise ValueError("unsupported upstream slice type")
            self.take("{")
            return self.sequence("}")
        if token == "{":
            fields = {}
            while self.peek() != "}":
                key = self.take()
                self.take(":")
                if key in fields:
                    raise ValueError("duplicate upstream struct field")
                fields[key] = self.value()
                if self.peek() != "}":
                    self.take(",")
            self.take("}")
            return fields
        if token in self.constants:
            return self.constants[token]
        self.take("(")
        return helper(token, self.sequence(")"))

    def parse(self):
        value = self.value()
        if self.peek() is not None:
            raise ValueError("unconsumed upstream literal")
        return value


def json_shape(value, fields):
    if isinstance(value, list):
        return [json_shape(item, fields) for item in value]
    if isinstance(value, dict):
        return {
            fields[key]: json_shape(item, fields)
            for key, item in value.items()
            if fields[key] != "-"
        }
    return value


def convert(source):
    raw_bytes = source.read_bytes()
    if hashlib.sha256(raw_bytes).hexdigest() != SOURCE_SHA256:
        raise ValueError("presets.go differs from the pinned Magpie source; review the update first")
    for filename, name in [("vertex.go", "VertexPreset"), ("azure.go", "AzurePreset"),
                           ("remote_magpie.go", "RemoteMagpiePreset")]:
        text = (source.parent / filename).read_text()
        if not re.search(rf'^const {name} = "{CONSTANTS[name]}"$', text, re.M):
            raise ValueError(f"unexpected upstream constant: {name}")
    raw = raw_bytes.decode("utf-8")
    constants = dict(CONSTANTS)
    plan_text = raw.split("var alibabaPlanModels = ", 1)[1].split("\n\n", 1)[0]
    constants["alibabaPlanModels"] = LiteralParser(plan_text, constants).parse()
    literal = raw.split("var presets = ", 1)[1].split("\n\nfunc bedrockChat(", 1)[0]
    # Public Go JSON tags govern names; Hosts is intentionally excluded upstream.
    fields = dict(re.findall(r'^\s*(\w+)\s+(?:\[\])?\w+\s+`json:"([^",]+)', raw, re.M))
    presets = json_shape(LiteralParser(literal, constants).parse(), fields)
    # Match upstream Presets(): sponsored entries first, otherwise source order.
    presets.sort(key=lambda preset: not preset.get("sponsored", False))
    if len({preset["id"] for preset in presets}) != len(presets):
        raise ValueError("duplicate upstream preset ID")
    payload = (json.dumps(presets, indent=2, ensure_ascii=False) + "\n").encode("utf-8")
    if hashlib.sha256(payload).hexdigest() != JSON_SHA256:
        raise ValueError("converted JSON differs from the reviewed snapshot")
    return payload, len(presets)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--upstream", type=Path, required=True,
                        help=f"local yetone/magpie checkout at {REVISION}; no network is used")
    parser.add_argument("--output", type=Path,
                        default=Path(__file__).resolve().parent.parent / "third_party/magpie/presets.json")
    args = parser.parse_args()
    payload, count = convert(args.upstream / "internal/provider/presets.go")
    args.output.write_bytes(payload)
    print(json.dumps({"presets": count, "bytes": len(payload), "sha256": JSON_SHA256,
                      "output": str(args.output)}))


if __name__ == "__main__":
    main()
