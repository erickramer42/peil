"""Writes the license notices of the crates linked into the `peil` binary.

    python -I packaging/crate_licenses.py --target <triple> --out <file>

The MIT License asks that its copyright and permission notice go with every
copy, so each binary Peil ships (the Linux tarballs, the core inside the APK)
carries this file. The crates are the ones `cargo metadata` resolves for the
target from Cargo.lock: normal dependencies of sensio-cli, followed through
normal dependencies only. Build scripts' dependencies and procedural macros
run on the build computer and are never linked, so they're left out. Every
crate is used under MIT (deny.toml), and the file holds each one's MIT text
as the crate ships it. Fails, naming the crate, when a crate doesn't offer
MIT or ships no MIT text: that needs a look, not a guess.

The output depends only on Cargo.lock, the target and the crates' sources,
so two runs give the same bytes.
"""

import argparse
import json
import pathlib
import re
import subprocess
import sys

ROOT = "sensio-cli"
# The sentence every MIT License text carries.
MIT_MARKER = "permission is hereby granted, free of charge"
LICENSE_FILE = re.compile(r"^(licen[cs]e|copying)", re.IGNORECASE)


def metadata(repo: pathlib.Path, target: str) -> dict:
    out = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--locked", "--filter-platform", target],
        cwd=repo, check=True, capture_output=True,
    ).stdout
    return json.loads(out)


def linked(meta: dict) -> list[dict]:
    """The packages linked into the root's binary, the workspace's own left out."""
    packages = {p["id"]: p for p in meta["packages"]}
    nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
    members = set(meta["workspace_members"])
    root = next(p["id"] for p in meta["packages"] if p["name"] == ROOT and p["id"] in members)

    def is_proc_macro(pid: str) -> bool:
        return any("proc-macro" in t["kind"] for t in packages[pid]["targets"])

    seen, todo = set(), [root]
    while todo:
        pid = todo.pop()
        if pid in seen:
            continue
        seen.add(pid)
        for dep in nodes[pid]["deps"]:
            normal = any(k["kind"] is None for k in dep["dep_kinds"])
            if normal and not is_proc_macro(dep["pkg"]):
                todo.append(dep["pkg"])
    found = [packages[pid] for pid in seen if pid not in members]
    return sorted(found, key=lambda p: (p["name"], p["version"]))


def offers_mit(expression: str | None) -> bool:
    return expression is not None and "MIT" in re.split(r"[\s()/]+", expression)


def mit_text(package: dict) -> str:
    """The crate's MIT text: a license file holding MIT's permission sentence,
    one named for MIT first."""
    folder = pathlib.Path(package["manifest_path"]).parent
    candidates = sorted(f for f in folder.iterdir() if f.is_file() and LICENSE_FILE.match(f.name))
    if package.get("license_file"):
        candidates.append(folder / package["license_file"])
    texts = []
    for f in candidates:
        text = f.read_text(encoding="utf-8", errors="replace").replace("\r\n", "\n")
        if MIT_MARKER in " ".join(text.lower().split()):
            texts.append(("mit" not in f.name.lower(), f.name, text.strip()))
    if not texts:
        raise SystemExit(f"{package['name']} {package['version']}: no MIT license text in the crate")
    return min(texts)[2]


def render(packages: list[dict], target: str) -> str:
    lines = [
        f"The Rust crates linked into peil for {target}, each used under the MIT License.",
        "Each crate's license text follows, as the crate ships it.",
        "",
    ]
    for p in packages:
        if not offers_mit(p.get("license")):
            raise SystemExit(f"{p['name']} {p['version']}: its license, {p.get('license')}, doesn't offer MIT")
        lines += ["=" * 72, f"{p['name']} {p['version']} ({p['license']})"]
        if p.get("repository"):
            lines.append(p["repository"])
        lines += ["", mit_text(p), ""]
    return "\n".join(lines)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--target", required=True, help="the Rust target triple")
    parser.add_argument("--out", required=True, type=pathlib.Path)
    args = parser.parse_args()
    repo = pathlib.Path(__file__).resolve().parent.parent
    packages = linked(metadata(repo, args.target))
    if not packages:
        raise SystemExit("no crates found; is this the right package?")
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_bytes(render(packages, args.target).encode("utf-8"))
    print(f"{args.out}: {len(packages)} crates", file=sys.stderr)


if __name__ == "__main__":
    main()
