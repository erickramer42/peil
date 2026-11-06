# ADR-0003: The product name only where a user sees it

Status: Accepted

## Context

A product name can change; the code's names, and the names inside files users keep, shouldn't have to change with it. Peil's sources also reach past RF (a vehicle's CAN bus, its battery while parked, a magnetometer), so an internal name tied to "RF" would be too narrow.

## Decision

1. **The product name, Peil, goes only where a user or builder sees it:** the README and docs, the command (`peil`), the CLI's and the HUD's titles, the repository (`peil-rf`), and later the configuration and data folders and any environment variables (`PEIL_*`).
2. **Everything internal uses the neutral name `sensio`** (sensor I/O): the crates (`sensio-core`, `sensio-sources`, `sensio-cli`, imported as `sensio_core` and so on) and any future C ABI (`sensio_` functions, `SENSIO_` constants).
3. **Identifiers inside saved files are neutral too:** a recording manifest's format ID (`sensio.recording/1`), schema names, and rule IDs (`persistence.places`). Recordings outlive a rename, so a product name in them would either stay forever or break every past recording.

## Consequences

- A rename touches only the places in item 1: the docs, the binary's name, the titles, the folders and the repository. GitHub redirects a renamed repository.
- Code and doc comments say "the core" or "the judge", not the product's name, except where they describe the command a user runs.
