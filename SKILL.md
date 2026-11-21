---
name: hongkong-bus-skill
description: Hong Kong bus routes, stop lists and live next-arrival ETAs from one Rust CLI and MCP server — KMB/LWB, Citybus, green minibus (GMB), New Lantao Bus (NLB) and MTR Bus, live Park Island coach arrivals, published routes and fares for private estate coaches such as Discovery Bay, and 16seats.net's red minibus route directory. Use for any question about Hong Kong buses, minibuses, stops or when the next bus arrives.
license: MIT
---

# Hong Kong buses

`hkbus` combines the five Hong Kong bus open-data feeds behind one command
and one MCP server. It needs no API key and no account. It is an independent,
unofficial project.

Use it when the user asks about:

- the next KMB, Citybus, green minibus, NLB or MTR Bus at a stop
- which stops a route serves and in what order
- which route goes from A to B in Hong Kong
- a Discovery Bay or Park Island coach
- which red minibus goes from Kennedy Town to Shau Kei Wan, or what it costs

## Before first use

```console
$ bash scripts/ensure-binary.sh
/Users/you/.local/bin/hkbus
```

```bash
BIN="$(bash scripts/ensure-binary.sh)"
```

The script checks `$HKBUS_BIN`, `hkbus` on `PATH`, then builds with
`cargo build --release --locked`. `scripts/install.sh` installs into
`~/.local/bin`.

### Install the binary directly

`npx skills add` installs the skill files. If the binary is missing, install it
with the release installer or cargo:

```bash
curl -fsSL https://raw.githubusercontent.com/hongkong-ai-labs/hongkong-bus-skill/main/scripts/install.sh | sh
# or
cargo install --git https://github.com/hongkong-ai-labs/hongkong-bus-skill --locked
```

Register it with any MCP client:

```bash
npx add-mcp "$(command -v hkbus) mcp" --name hongkong-bus -a claude-code -a codex -a cursor -y
claude mcp add hongkong-bus -- hkbus mcp
codex mcp add hongkong-bus -- hkbus mcp
```

`add-mcp` detects the client configuration files in the current project; add
`-g` for a global install. `$(command -v hkbus)` keeps the entry correct
when the binary is on `PATH`; substitute an absolute path if your client does
not inherit the shell `PATH`.

## CLI

```bash
$BIN routes kmb --query 1
$BIN routes ctb --query "Happy Valley"
$BIN routes gmb --region HKI --query 1
$BIN routes nlb --query "Tai O"
$BIN routes mtrbus --query K51

$BIN stops kmb 1 --direction outbound
$BIN stops ctb 1
$BIN stops gmb 1
$BIN stops nlb 1
$BIN stops mtrbus K51

$BIN eta kmb 1 "Chuk Yuen"
$BIN eta kmb 1 "Chuk Yuen" --direction outbound --json
$BIN eta ctb 1 "Central"
$BIN eta gmb N40 "Stanley Beach"
$BIN eta nlb 1 "Mui Wo"
$BIN eta mtrbus K51 K51-U210

$BIN private
$BIN private --query "Discovery Bay"
$BIN private-eta NR330
$BIN private-eta NR330 --direction 1 --json
$BIN red-minibus --query "Kennedy Town"
$BIN red-minibus --district "Sha Tin"
$BIN red-route r_h08
$BIN update --check
```

Operators are `kmb` (KMB and LWB), `ctb` (Citybus), `gmb` (green minibus),
`nlb` (New Lantao Bus) and `mtrbus` (MTR Bus). `--json` works globally.

### Routes

`routes` searches the operator's published route list by route number, origin
or destination. Use `--limit` to cap the output. Green minibus route lists are
split by region: `HKI`, `KLN` or `NT`; without `--region` all three are
searched.

KMB publishes a direction (`O`/`I`) and a service type for each variant. Most
routes have service type `1`; some have short-working or overnight variants
with another type. The `stops` and `eta` commands use every matching variant
unless `--direction` narrows it.

### Stops and ETAs

`stops` prints stop sequence, stop id, name and direction. Use the id for an
exact `eta` lookup, or a distinctive part of the name. The first matching stop
is used, so a name that appears several times on a route should be narrowed
with `--direction` or replaced by its id.

`eta` returns minutes until arrival and the operator's remark (`Scheduled`,
`Final Bus`, `Arriving / Departed`, and so on). An empty result is common late
at night or at the end of a route; it does not mean the route does not exist.
The renderer shows `no ETA` when the operator has a stop but no prediction.

MTR Bus publishes stop ids but not stop names. `stops mtrbus` therefore shows
the id in both columns, and `eta mtrbus` matches on the id.

### Private coaches and red minibuses

`private` lists the external routes and published adult fares of the largest
private estate operators:

- **Discovery Bay (DBTSL)** — DB01A / DB01R Tung Chung, DB02A / DB02R airport,
  DB03P / DB03R Sunny Bay, and DB08R Central overnight. Resident and regular
  fares are shown where DBTSL publishes them.
- **Park Island (PITCL)** — NR330 Tsing Yi, NR332 Kwai Fong, NR334 airport,
  NR338 Central overnight and NR338S Central via Hong Kong Station. NR331 /
  NR331S are Sun Bus routes to Tsuen Wan; NR330A is the peak-hour Tsing Yi
  supplement.

Park Island publishes live arrivals through its public ETA backend.
`private-eta NR330` (also NR332, NR334, NR338) returns minutes to each stop for
directions 1 and 2. Discovery Bay publishes timetables but not a public ETA
feed, so this skill deliberately does not invent departure times there. Give
the user the operator timetable for exact Discovery Bay times.

Red minibuses are not scheduled, but they do have recognised route corridors.
`red-minibus` lists routes scraped from the 16seats.net public directory, with
district, service description and route code. Filters are `--query` (route or
place) and `--district`. `red-route <code or name>` returns the published page:
service period, fare table and major boarding/alighting points.

Fares and routeing remain at the driver's discretion, and there is no live ETA.
Quote the published fare as a range and tell the passenger to check the
windscreen. Never invent a red minibus departure time.

## MCP

```bash
$BIN mcp
```

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

Tools:

| Tool | Arguments |
| --- | --- |
| `bus_routes` | `{ "operator": "kmb"\|"ctb"\|"gmb"\|"nlb"\|"mtrbus", "query"?: string, "region"?: "HKI"\|"KLN"\|"NT", "limit"?: number }` |
| `bus_stops` | `{ "operator": string, "route": string, "direction"?: string }` |
| `bus_eta` | `{ "operator": string, "route": string, "stop": string, "direction"?: string }` |
| `private_bus_routes` | `{ "query"?: string }` |
| `private_bus_eta` | `{ "route": "NR330"\|"NR332"\|"NR334"\|"NR338", "direction"?: 1\|2 }` |
| `red_minibus_routes` | `{ "query"?: string, "district"?: string }` |
| `red_minibus_route` | `{ "route": string }` |

The server speaks JSON-RPC 2.0 over stdio, one message per line, and implements
`initialize`, `ping`, `tools/list` and `tools/call`.

## Cautions

- ETA feeds are predictions. Traffic, weather and special arrangements can
  change them without notice.
- A route number is not unique across operators. Say which operator you used
  when reporting a result.
- KMB and Citybus stops are often paired by direction; check the direction and
  the destination before telling someone a bus is coming.
- Fares are not exposed consistently across the live feeds. NLB publishes a
  fare per stop; the others do not. Private-coach and red-minibus fares come
  from published reference pages and can change at the operator's discretion.
- This is not an official KMB, Citybus, GMB, NLB, MTR, PITCL or Transport
  Department channel.

## Sources

- KMB/LWB: `https://data.etabus.gov.hk/v1/transport/kmb/`
- Citybus: `https://rt.data.gov.hk/v2/transport/citybus/`
- Green minibus: `https://data.etagmb.gov.hk/`
- Red minibus directory: `https://www.16seats.net/eng/`
- Discovery Bay: `https://www.visitdiscoverybay.com/en/getting-around`
- Park Island live ETA: `https://360-api.socif.co/api/eta/route-stop/{route}/{direction}`
- Park Island: `http://www.pitcl.com.hk/en/bus_service.php`
- NLB: `https://rt.data.gov.hk/v2/transport/nlb/`
- MTR Bus: `POST https://rt.data.gov.hk/v1/transport/mtr/bus/getSchedule`
- Red minibus operating rules: `https://www.td.gov.hk/en/transport_in_hong_kong/public_transport/minibuses/index.html`

Responses are cached in-process for 30 seconds (ETA), one hour (route and stop
lists), six hours (red minibus pages) or one day (stop names).
