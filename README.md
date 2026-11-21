# hongkong-bus-skill

Hong Kong bus routes, stops and live next-arrival times in one Rust CLI and
MCP server.

| Operator | Routes | Stops | Live ETA |
| --- | --- | --- | --- |
| KMB / LWB | yes | yes | yes |
| Citybus | yes | yes | yes |
| Green minibus (GMB) | yes | yes | yes |
| New Lantao Bus (NLB) | yes | yes | yes |
| MTR Bus | yes | ids only | yes |
| Discovery Bay (DBTSL) | reference | no | no |
| Park Island (PITCL) | reference | no | NR330/332/334/338 |
| Red minibus (16seats.net) | yes | major points | no |

## Install

**Agent skill** — installs `SKILL.md` and the binary bootstrap script:

```bash
npx skills add hongkong-ai-labs/hongkong-bus-skill
```

**Prebuilt binary** — macOS (Intel or Apple silicon) and Linux (x86_64 or
aarch64, static musl), checksum-verified, into `~/.local/bin`:

```bash
curl -fsSL https://raw.githubusercontent.com/hongkong-ai-labs/hongkong-bus-skill/main/scripts/install.sh | sh
```

**From source** — Rust 1.89 or newer:

```bash
git clone https://github.com/hongkong-ai-labs/hongkong-bus-skill
cd hongkong-bus-skill
cargo install --path . --locked
# or: cargo build --release --locked   # target/release/hkbus
```

Verify:

```console
$ hkbus --version
```

## Usage

```bash
hkbus routes kmb --query 1
hkbus routes gmb --region HKI --query 1
hkbus stops kmb 1 --direction outbound
hkbus eta kmb 1 "Chuk Yuen"
hkbus eta ctb 1 "Central"
hkbus eta gmb N40 "Stanley Beach"
hkbus eta nlb 1 "Mui Wo"
hkbus eta mtrbus K51 K51-U210
hkbus private --query "Discovery Bay"
hkbus private-eta NR330
hkbus red-minibus --query "Kennedy Town"
hkbus red-minibus --district "Sha Tin"
hkbus red-route r_h08
```

`--json` before the subcommand prints structured output.

## Notes

- KMB routes carry a direction and service type. `stops` and `eta` cover every
  matching variant unless `--direction` narrows them.
- Green minibus route numbers are regional; `--region HKI|KLN|NT` avoids
  ambiguity.
- An empty ETA list is normal at night or at a route's terminus. The CLI says
  `no ETA` when a stop exists but has no prediction.
- MTR Bus publishes stop ids only. The feed does not include stop names.
- Discovery Bay and Park Island routes and fares are reference data. Park
  Island NR330/332/334/338 have live ETA via `private-eta`; Discovery Bay has
  no public live arrival feed.
- Red minibuses are not scheduled, but `red-minibus` lists recognised routes
  from the 16seats.net directory and `red-route` shows the published fares and
  major boarding points. There is still no live ETA.

## MCP

```json
{
  "mcpServers": {
    "hongkong-bus": {
      "command": "/absolute/path/to/hkbus",
      "args": ["mcp"]
    }
  }
}
```

Tools: `bus_routes`, `bus_stops`, `bus_eta`, `private_bus_routes`,
`private_bus_eta`, `red_minibus_routes`, `red_minibus_route`. See
[`SKILL.md`](SKILL.md) for schemas.

## Run modes and deployment

| Mode | Start with | Use for |
| --- | --- | --- |
| CLI | `hkbus <command>` | Interactive use and shell scripts |
| JSON | `<command>` with `--json` | `jq`, automation, other programs |
| MCP stdio | `hkbus mcp` | Desktop and CLI MCP clients |
| Container | `docker run --rm -i hkbus mcp` | No local Rust toolchain |

### MCP and authentication

This server is **stdio-only**: the client launches the process and speaks MCP
over stdin/stdout. There is no listening port, so there is no network
authentication layer and no API-key setting. Optional API keys in this project
family apply only to servers that expose an HTTP transport. Keep the process
local and let the MCP client manage it.

Typical client entry:

```json
{
  "mcpServers": {
    "hongkong-bus-skill": {
      "command": "hkbus",
      "args": ["mcp"]
    }
  }
}
```

If the client does not inherit `PATH`, use the absolute installed path.

### Docker and Apple container

```bash
docker build -t hkbus .
docker run --rm hkbus --help
docker run --rm -i hkbus mcp          # stdio MCP
docker compose run --rm stdio             # same, through Compose
```

Apple's `container` runtime uses the same OCI image:

```bash
container build -t hkbus .
container run --rm hkbus --help
container run --rm -i hkbus mcp
```

The runtime image contains the release binary and CA certificates, runs as a
non-root user, and defaults to `--help` so a bare `docker run` is harmless.

### Cloudflare and WASM

There is no Worker or WASM deployment. The binary talks directly to upstream
HTTP APIs over TLS and uses a Tokio runtime; packaging it as a stdio server is
the supported integration. A Worker would need a separate HTTP/JavaScript
adapter and is out of scope for this repo.

### Releases

Pushing a `v*` tag runs the release workflow, which checks the tag against
`Cargo.toml` and `CHANGELOG.md`, builds four targets, smoke-tests native
binaries and publishes:

- `hkbus-<tag>-<target>.tar.gz` for macOS and static Linux
- `SHA256SUMS` covering every tarball

`scripts/install.sh` downloads the latest release, verifies its checksum and
falls back to `cargo build --release --locked` when no release is available.
`just tag X.Y.Z` creates the annotated tag locally; `just release X.Y.Z` tags
and pushes to trigger the workflow.

## Development

```bash
just                 # fmt check, clippy with -D warnings, tests
just skill-check     # validate SKILL.md and shell syntax
just container       # build the image and print CLI help
cargo test
cargo clippy -- -D warnings
cargo fmt --check
```

## Licence

MIT. Independent project, not affiliated with any operator or the Transport
Department.
