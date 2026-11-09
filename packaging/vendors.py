#!/usr/bin/env python3
"""Refresh the vendor registry snapshot the core bundles (ADR-0006 section 5).

Downloads the IEEE Registration Authority's three public listings (MA-L,
MA-M, MA-S), keeps each row's registry, assignment and organization name
(the parser reads nothing else; the postal address would double the size),
sorts them, and writes them as one CSV beside a text file naming the date
and the hash of each list as downloaded.

    python -I packaging/vendors.py              # download and write
    python -I packaging/vendors.py --from DIR   # from oui.csv, mam.csv and
                                                # oui36.csv already in DIR

The snapshot changes vendor names on replay, and the hash in every
`emitters` line shows which snapshot described; refresh it with a release.
"""

from __future__ import annotations

import argparse
import csv
import datetime as dt
import hashlib
import io
import sys
import urllib.request
from pathlib import Path

LISTS = (
    ("MA-L", "oui.csv", "https://standards-oui.ieee.org/oui/oui.csv"),
    ("MA-M", "mam.csv", "https://standards-oui.ieee.org/oui28/mam.csv"),
    ("MA-S", "oui36.csv", "https://standards-oui.ieee.org/oui36/oui36.csv"),
)
HEADER = ("Registry", "Assignment", "Organization Name", "Organization Address")
# The parser's own bounds (describe.rs): an organization name fits 256 bytes.
MAX_ORGANIZATION = 256
HEX = set("0123456789ABCDEF")
PREFIX_LENGTH = {"MA-L": 6, "MA-M": 7, "MA-S": 9}
MAX_BYTES = 64 << 20


def fetch(url: str) -> bytes:
    with urllib.request.urlopen(url, timeout=120) as response:  # noqa: S310 (fixed https URLs)
        data = response.read(MAX_BYTES + 1)
    if len(data) > MAX_BYTES:
        raise SystemExit(f"{url}: longer than 64 MiB")
    return data


def rows(registry: str, data: bytes, name: str) -> list[tuple[str, str, str]]:
    """The (registry, assignment, organization) rows of one list, checked."""
    text = data.decode("utf-8")
    reader = csv.reader(io.StringIO(text, newline=""))
    header = next(reader, None)
    if header is None or tuple(header) != HEADER:
        raise SystemExit(f"{name}: unexpected header {header!r}")
    out = []
    for number, row in enumerate(reader, start=2):
        if not row:
            continue
        if len(row) != 4:
            raise SystemExit(f"{name} line {number}: {len(row)} fields, not 4")
        reg, assignment, organization, _address = row
        if reg != registry:
            raise SystemExit(f"{name} line {number}: registry {reg!r}, not {registry}")
        if len(assignment) != PREFIX_LENGTH[registry] or not set(assignment) <= HEX:
            raise SystemExit(f"{name} line {number}: assignment isn't {PREFIX_LENGTH[registry]} hex digits")
        organization = " ".join(organization.split())
        if not organization or len(organization.encode()) > MAX_ORGANIZATION:
            raise SystemExit(f"{name} line {number}: organization name empty or over {MAX_ORGANIZATION} bytes")
        out.append((reg, assignment, organization))
    return out


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--from", dest="source", type=Path, help="a directory holding the three lists already downloaded")
    parser.add_argument(
        "--out",
        type=Path,
        default=Path(__file__).resolve().parent.parent / "crates" / "sensio-core" / "data" / "ieee-vendors.csv",
    )
    args = parser.parse_args()

    all_rows: list[tuple[str, str, str]] = []
    sources = []
    for registry, filename, url in LISTS:
        data = (args.source / filename).read_bytes() if args.source else fetch(url)
        digest = hashlib.sha256(data).hexdigest()
        listed = rows(registry, data, filename)
        all_rows.extend(listed)
        sources.append(f"{registry}: {url}\n    sha256 {digest}, {len(listed)} assignments")
    # A few of the oldest MA-L blocks are listed under several organizations;
    # one row names them all, so a lookup says so instead of picking one.
    by_assignment: dict[tuple[str, str], set[str]] = {}
    for registry, assignment, organization in all_rows:
        by_assignment.setdefault((registry, assignment), set()).add(organization)
    all_rows = sorted((r, a, " / ".join(sorted(names))) for (r, a), names in by_assignment.items())
    if any(len(organization.encode()) > MAX_ORGANIZATION for _, _, organization in all_rows):
        raise SystemExit("a joined organization name is over 256 bytes")

    args.out.parent.mkdir(parents=True, exist_ok=True)
    with args.out.open("w", encoding="utf-8", newline="") as f:
        writer = csv.writer(f, lineterminator="\n")
        writer.writerow(HEADER[:3])
        writer.writerows(all_rows)
    today = dt.datetime.now(dt.timezone.utc).date().isoformat()
    notes = args.out.with_name("ieee-vendors.sources.txt")
    notes.write_text(
        "The IEEE Registration Authority's public listings, read "
        f"{today}, cut to registry, assignment and organization name and sorted\n"
        "by packaging/vendors.py. The lists as downloaded:\n\n" + "\n".join(sources) + "\n",
        encoding="utf-8",
        newline="\n",
    )
    print(f"{args.out}: {len(all_rows)} assignments; sources in {notes.name}")


if __name__ == "__main__":
    sys.exit(main())
