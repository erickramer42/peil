#!/usr/bin/env python3
"""Make the aircraft registry snapshot the core bundles (ADR-0007 section 5).

Reads the FAA's releasable aircraft database as downloaded
(https://registry.faa.gov/database/ReleasableAircraft.zip; the site turns
scripted downloads away, so it's downloaded by hand) and keeps, for each
registered aircraft, its Mode S address, N-number, manufacturer and model,
year built, aircraft and engine type, and registrant type. The registrant's
name is kept only for a corporation, a government, a limited liability
company or a non-citizen corporation; a person's name, co-owners' and
partners' names (OTHER NAMES) and every address are dropped here, so they
never reach the repository. An aircraft that isn't type-certificated (an
amateur build, say) has its builder as its manufacturer, often a person, so
its make and model are the kit's (KIT MFR, KIT MODEL) where it lists one,
and otherwise none.

    python -I packaging/faa_registry.py ReleasableAircraft.zip

The snapshot is rows sorted by address in blocks of 512, each block raw
deflate on its own, so a lookup inflates one block (`aircraft_registry.rs`
reads the layout written by `write`). A new snapshot changes descriptions on
replay: make it only for a minor release, with `DESCRIBE_VERSION` bumped.
"""

from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import io
import struct
import sys
import zipfile
import zlib
from pathlib import Path

URL = "https://registry.faa.gov/database/ReleasableAircraft.zip"
MAGIC = b"sensio.faa/1\n"
BLOCK_ROWS = 512
# The reader's bounds (aircraft_registry.rs): a field fits 64 bytes, a block 64 KiB inflated.
MAX_FIELD = 64
MAX_BLOCK = 64 << 10
MAX_MEMBER = 512 << 20
# TYPE REGISTRANT codes (ardata.pdf, 5/8/2025): 3 corporation, 5 government,
# 7 LLC, 8 non-citizen corporation. 1 individual, 2 partnership, 4 co-owned
# and 9 non-citizen co-owned name people.
ORGANIZATIONS = {"3", "5", "7", "8"}
REGISTRANTS = {"", "1", "2", "3", "4", "5", "7", "8", "9"}
AIRCRAFT_TYPES = {"1", "2", "3", "4", "5", "6", "7", "8", "9", "H", "O"}
ENGINE_TYPES = {str(n) for n in range(12)}
MASTER_COLUMNS = ("MODE S CODE HEX", "N-NUMBER", "MFR MDL CODE", "YEAR MFR", "TYPE REGISTRANT", "NAME",
                  "TYPE AIRCRAFT", "TYPE ENGINE", "CERTIFICATION", "KIT MFR", "KIT MODEL")
ACFTREF_COLUMNS = ("CODE", "MFR", "MODEL", "BUILD-CERT-IND")
# BUILD-CERT-IND (ardata.pdf): 0 type certificated, 1 not type certificated,
# 2 light sport. A make is a company's for 0, and for 2 outside the
# experimental class (CERTIFICATION's first character 4).
TYPE_CERTIFICATED = "0"
LIGHT_SPORT = "2"
EXPERIMENTAL = "4"


def table(archive: zipfile.ZipFile, member: str, wanted: tuple[str, ...]):
    """Each row of a comma-separated member as a dict of the wanted columns."""
    if archive.getinfo(member).file_size > MAX_MEMBER:
        raise SystemExit(f"{member}: larger than 512 MiB")
    with archive.open(member) as f:
        reader = io.TextIOWrapper(f, encoding="utf-8-sig")
        columns = [c.strip() for c in reader.readline().split(",")]
        missing = [c for c in wanted if c not in columns]
        if missing:
            raise SystemExit(f"{member}: no column {missing[0]!r}")
        at = {c: columns.index(c) for c in wanted}
        for number, line in enumerate(reader, start=2):
            fields = line.rstrip("\r\n").split(",")
            if len(fields) != len(columns):
                raise SystemExit(f"{member} line {number}: {len(fields)} fields, not {len(columns)}")
            yield number, {c: " ".join(fields[i].split()) for c, i in at.items()}


def field(text: str, where: str) -> str:
    if "\t" in text or "\n" in text or len(text.encode()) > MAX_FIELD:
        raise SystemExit(f"{where}: a field with a tab, a newline or over {MAX_FIELD} bytes")
    return text


def rows(archive: zipfile.ZipFile) -> list[tuple[int, list[str]]]:
    models = {}
    for number, row in table(archive, "ACFTREF.txt", ACFTREF_COLUMNS):
        where = f"ACFTREF.txt line {number}"
        models[row["CODE"]] = (field(row["MFR"], where), field(row["MODEL"], where), row["BUILD-CERT-IND"])
    out = {}
    for number, row in table(archive, "MASTER.txt", MASTER_COLUMNS):
        where = f"MASTER.txt line {number}"
        hex_address = row["MODE S CODE HEX"]
        if len(hex_address) != 6 or any(c not in "0123456789abcdefABCDEF" for c in hex_address):
            raise SystemExit(f"{where}: MODE S CODE HEX isn't 6 hex digits")
        address = int(hex_address, 16)
        if address in out:
            raise SystemExit(f"{where}: an address listed twice")
        n_number = row["N-NUMBER"]
        if not (1 <= len(n_number) <= 5 and n_number.isascii() and n_number.isalnum()):
            raise SystemExit(f"{where}: N-NUMBER isn't 1-5 letters and digits")
        year = row["YEAR MFR"]
        if year and not (len(year) == 4 and year.isdigit()):
            raise SystemExit(f"{where}: YEAR MFR isn't 4 digits")
        registrant, aircraft, engine = row["TYPE REGISTRANT"], row["TYPE AIRCRAFT"], row["TYPE ENGINE"]
        if registrant not in REGISTRANTS or aircraft not in AIRCRAFT_TYPES or engine not in ENGINE_TYPES:
            raise SystemExit(f"{where}: a type code outside ardata.pdf's")
        make, model, build = models.get(row["MFR MDL CODE"], ("", "", ""))
        certified = build == TYPE_CERTIFICATED or (
            build == LIGHT_SPORT and not row["CERTIFICATION"].startswith(EXPERIMENTAL)
        )
        if not certified:
            make, model = field(row["KIT MFR"], where), field(row["KIT MODEL"], where)
        name = field(row["NAME"], where) if registrant in ORGANIZATIONS else ""
        out[address] = [n_number, make, model, year, aircraft, engine, registrant, name]
    return sorted(out.items())


def write(rows: list[tuple[int, list[str]]]) -> bytes:
    """The snapshot: MAGIC, the block count (u32), then per block its first
    address and the end of its bytes in the data (u32 each), then the data.
    A block's lines are tab-separated: the address less the previous line's
    (the block's first address for its first line), then the kept fields."""
    index, data = [], bytearray()
    for start in range(0, len(rows), BLOCK_ROWS):
        block = rows[start:start + BLOCK_ROWS]
        previous = block[0][0]
        lines = []
        for address, fields in block:
            lines.append("\t".join([str(address - previous), *fields]))
            previous = address
        text = ("\n".join(lines) + "\n").encode()
        if len(text) > MAX_BLOCK:
            raise SystemExit(f"a block of {len(text)} bytes, over {MAX_BLOCK}")
        deflate = zlib.compressobj(9, zlib.DEFLATED, -15)
        data += deflate.compress(text) + deflate.flush()
        index.append((block[0][0], len(data)))
    head = MAGIC + struct.pack("<I", len(index)) + b"".join(struct.pack("<II", a, end) for a, end in index)
    return head + bytes(data)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("zip", type=Path, help="ReleasableAircraft.zip as downloaded")
    parser.add_argument(
        "--out",
        type=Path,
        default=Path(__file__).resolve().parent.parent / "crates" / "sensio-core" / "data" / "faa-aircraft.bin",
    )
    args = parser.parse_args()

    digest = hashlib.sha256(args.zip.read_bytes()).hexdigest()
    with zipfile.ZipFile(args.zip) as archive:
        dated = max(dt.date(*archive.getinfo(m).date_time[:3]) for m in ("MASTER.txt", "ACFTREF.txt"))
        listed = rows(archive)
    snapshot = write(listed)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_bytes(snapshot)
    named = sum(1 for _, fields in listed if fields[-1])
    notes = args.out.with_name("faa-aircraft.sources.txt")
    notes.write_text(
        f"The FAA's releasable aircraft database, {URL}, its files dated {dated.isoformat()},\n"
        "cut by packaging/faa_registry.py to each aircraft's Mode S address, N-number, make and\n"
        "model, year, aircraft and engine type and registrant type, with the registrant's name\n"
        "for organizations only. Field codes from the FAA's ardata.pdf (dated 5/8/2025).\n\n"
        f"ReleasableAircraft.zip: sha256 {digest}\n"
        f"    {len(listed)} aircraft, {named} with an organization's name; {len(snapshot)} bytes\n",
        encoding="utf-8",
        newline="\n",
    )
    print(f"{args.out}: {len(listed)} aircraft, {len(snapshot)} bytes; sources in {notes.name}")


if __name__ == "__main__":
    sys.exit(main())
