"""The release workflow's checks and notes.

    python -I packaging/release.py check <tag>
    python -I packaging/release.py notes <tag> <cert-sha256> <file>...

`check` passes when the tag names the version every part of the build
carries: the workspace's version in Cargo.toml and the app's major, minor and
patch in android/app/build.gradle.kts. A release tag (`v0.1.0`) needs a dated
CHANGELOG.md section for its version (`## 0.1.0 - 2026-11-08`); a dry run's
tag (`v0.1.0-rc1`) needs only a section, dated or `Unreleased`, and its
release is marked a prerelease. It prints `version=` and `prerelease=` lines
for the workflow's outputs.

`notes` writes the release notes to stdout: the downloads with their SHA-256,
the APK's certificate fingerprint, and the version's CHANGELOG.md section.
"""

import hashlib
import pathlib
import re
import sys
import tomllib

REPO = pathlib.Path(__file__).resolve().parent.parent
TAG = re.compile(r"^v(\d+\.\d+\.\d+)(-rc\d+)?$")
DATE = re.compile(r"^\d{4}-\d{2}-\d{2}$")


def fail(message: str) -> None:
    raise SystemExit(message)


def parse_tag(tag: str) -> tuple[str, bool]:
    m = TAG.match(tag)
    if not m:
        fail(f"tag {tag!r} isn't vMAJOR.MINOR.PATCH or vMAJOR.MINOR.PATCH-rcN")
    return m.group(1), m.group(2) is not None


def cargo_version() -> str:
    with open(REPO / "Cargo.toml", "rb") as f:
        return tomllib.load(f)["workspace"]["package"]["version"]


def app_version() -> str:
    text = (REPO / "android/app/build.gradle.kts").read_text(encoding="utf-8")
    parts = []
    for name in ("major", "minor", "patch"):
        m = re.search(rf"^val {name} = (\d+)$", text, re.MULTILINE)
        if not m:
            fail(f"android/app/build.gradle.kts has no 'val {name} = N' line")
        parts.append(m.group(1))
    return ".".join(parts)


def changelog_section(version: str) -> tuple[str, str]:
    """The version's heading suffix (a date or `Unreleased`) and its body."""
    text = (REPO / "CHANGELOG.md").read_text(encoding="utf-8").replace("\r\n", "\n")
    sections = re.split(r"^## ", text, flags=re.MULTILINE)[1:]
    for section in sections:
        heading, _, body = section.partition("\n")
        number, _, when = heading.partition(" - ")
        if number.strip() == version:
            return when.strip(), body.strip()
    fail(f"CHANGELOG.md has no '## {version} - <date>' section")


def check(tag: str) -> None:
    version, prerelease = parse_tag(tag)
    if (got := cargo_version()) != version:
        fail(f"tag {tag} but Cargo.toml's workspace version is {got}")
    if (got := app_version()) != version:
        fail(f"tag {tag} but the app's version is {got}")
    when, body = changelog_section(version)
    if not body:
        fail(f"CHANGELOG.md's {version} section is empty")
    if not prerelease and not DATE.match(when):
        fail(f"CHANGELOG.md's {version} section is dated {when!r}; a release needs YYYY-MM-DD")
    if not (prerelease or DATE.match(when) or when == "Unreleased"):
        fail(f"CHANGELOG.md's {version} section is dated {when!r}: YYYY-MM-DD or Unreleased")
    print(f"version={version}")
    print(f"prerelease={'true' if prerelease else 'false'}")


def notes(tag: str, cert: str, files: list[str]) -> None:
    version, _ = parse_tag(tag)
    _, body = changelog_section(version)
    out = ["## Downloads", "", "| File | SHA-256 |", "|---|---|"]
    for name in sorted(files):
        digest = hashlib.sha256(pathlib.Path(name).read_bytes()).hexdigest()
        out.append(f"| `{pathlib.Path(name).name}` | `{digest}` |")
    out += [
        "",
        "The APK is signed with APK Signature Scheme v3. Its certificate's SHA-256 fingerprint,",
        "which `apksigner verify --print-certs` shows, is:",
        "",
        f"`{cert}`",
        "",
        "## Changes",
        "",
        body,
        "",
    ]
    sys.stdout.write("\n".join(out))


def main(argv: list[str]) -> None:
    if len(argv) == 2 and argv[0] == "check":
        check(argv[1])
    elif len(argv) >= 4 and argv[0] == "notes":
        notes(argv[1], argv[2], argv[3:])
    else:
        fail(__doc__)


if __name__ == "__main__":
    main(sys.argv[1:])
