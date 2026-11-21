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
| Park Island (PITCL) | reference | no | no |
| Red minibus | no fixed routes | no | no |

## Install

Requires Rust 1.89 or newer.

```bash
cargo build --release --locked
./scripts/install.sh
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
hkbus red-minibus
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
- Discovery Bay and Park Island coaches are reference data. They have no
  public live ETA feed, so no times are invented here.
- Red minibuses have no fixed route, stop list or ETA dataset. The CLI explains
  how they work instead of pretending otherwise.

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
`red_minibus_info`. See [`SKILL.md`](SKILL.md) for schemas.

## Development

```bash
cargo test
cargo clippy -- -D warnings
cargo fmt --check
```

## Licence

MIT. Independent project, not affiliated with any operator or the Transport
Department.
