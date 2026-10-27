# acnl-treasuretown-mngr

A CLI-based manager for making Treasure Towns for Animal Crossing: New Leaf!

## What this project is

`acnl-treasuretown-mngr` is a Linux-first CLI manager for ACNL treasure-town operations:

- Isolated town creation, configuration, and lifecycle management
- Save import (`garden.dat`, `garden_plus.dat`), backups, snapshots, restores, and restocking
- Session state tracking for manual (physical 3DS) and external process (emulator) hosting backends
- Safe filesystem operations with path validation, lock files, and snapshot integrity hashing

## Install

```bash
git clone https://github.com/acer51-doctom/acnl-treasuretown-mngr.git
cd acnl-treasuretown-mngr
cargo build --release
./target/release/acnl-treasuretown-mngr --help
```

## Command overview

```bash
acnl-treasuretown-mngr --help
acnl-treasuretown-mngr version
acnl-treasuretown-mngr doctor

acnl-treasuretown-mngr town create <name>
acnl-treasuretown-mngr town list
acnl-treasuretown-mngr town info <name>
acnl-treasuretown-mngr town delete <name>
acnl-treasuretown-mngr town import-save <name> <path>
acnl-treasuretown-mngr town backup <name>
acnl-treasuretown-mngr town snapshots <name>
acnl-treasuretown-mngr town restore <name> <snapshot>
acnl-treasuretown-mngr town restock <name>

acnl-treasuretown-mngr start <name>
acnl-treasuretown-mngr stop <name>
acnl-treasuretown-mngr restart <name>
acnl-treasuretown-mngr status <name>
acnl-treasuretown-mngr status --all
acnl-treasuretown-mngr logs <name>
```

## Data layout

On Linux, runtime data is stored in:

```text
~/.local/share/acnl-treasuretown-mngr/
├── config.toml
├── towns/
│   └── <town-name>/
│       ├── town.toml
│       ├── saves/
│       ├── snapshots/
│       ├── logs/
│       └── runtime/
└── templates/
```

## Architecture modules

- `src/cli.rs`: CLI command parsing and command topology
- `src/config.rs`: app path detection, config persistence
- `src/town.rs`: town management, snapshot and session workflows
- `src/hosting.rs`: hosting backend abstraction + implementations
- `src/process_monitor.rs`: process health and termination helpers
- `src/storage.rs`: atomic writes, locking, hashing, save copy safety
- `src/logging.rs`: structured logging bootstrap

## Hosting backends

- **Manual backend**: tracks sessions hosted on a physical 3DS; does not claim remote control.
- **External process backend**: supervises a configured emulator process, captures stdout/stderr logs, records PID, and detects dead processes during status checks.

The current implementation does **not** assume unsupported Azahar CLI flags and does **not** claim automatic Pretendo connection detection.

## Session states

Towns track session state transitions using:

- `STOPPED`
- `STARTING`
- `RUNNING`
- `STOPPING`
- `ERROR`

Runtime metadata includes PID (when applicable), session timing, restart attempts, and last error.

Default session limit is 45 minutes (configurable) to support realistic ACNL/Pretendo session stability constraints.

## Current limitations

- Pretendo connection state, gate status, and visitor counts are currently unknown/undetected.
- External process crashes are detected by checking recorded PIDs during status checks, not by a persistent daemon.
- Item ordering, queue orchestration, and account isolation automation are not yet implemented.

## Future features (documented only)

- Headless Azahar backend integration
- Pretendo connectivity monitoring
- Automatic ACNL gate opening
- Live visitor counts
- Automated item placement
- Item ordering/catalog workflows
- Automated post-visitor restocking
- Web dashboard + REST API
- Discord integration
- Multi-host scheduling

## Example config

See `/examples/config.example.toml`.

## License

This repository currently includes a `LICENSE` file. Validate it matches your intended project license before first public release.
