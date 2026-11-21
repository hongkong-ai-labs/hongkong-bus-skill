//! Minimal MCP server over stdio.

use anyhow::Result;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::api::{Client, Operator, RED_MINIBUS};
use crate::render;

pub async fn serve(client: Client) -> Result<()> {
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut stdout = tokio::io::stdout();
    while let Some(line) = lines.next_line().await? {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let request = match serde_json::from_str::<Value>(line) {
            Ok(request) => request,
            Err(error) => {
                let response = json!({
                    "jsonrpc": "2.0",
                    "id": null,
                    "error": {"code": -32700, "message": format!("parse error: {error}")}
                });
                stdout
                    .write_all(format!("{}\n", serde_json::to_string(&response)?).as_bytes())
                    .await?;
                stdout.flush().await?;
                continue;
            }
        };
        if request.get("id").is_none() {
            continue;
        }
        let id = request["id"].clone();
        let method = request["method"].as_str().unwrap_or_default();
        let response = match method {
            "initialize" => result(
                id,
                json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name": "hongkong-bus", "version": env!("CARGO_PKG_VERSION")}
                }),
            ),
            "ping" => result(id, json!({})),
            "tools/list" => result(id, json!({"tools": tools()})),
            "tools/call" => match call(&client, &request).await {
                Ok(text) => result(
                    id,
                    json!({"content": [{"type": "text", "text": text}], "isError": false}),
                ),
                Err(error) => result(
                    id,
                    json!({
                        "content": [{"type": "text", "text": format!("{error:#}")}],
                        "isError": true
                    }),
                ),
            },
            other => json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {"code": -32601, "message": format!("method not found: {other}")}
            }),
        };
        stdout
            .write_all(format!("{}\n", serde_json::to_string(&response)?).as_bytes())
            .await?;
        stdout.flush().await?;
    }
    Ok(())
}

fn result(id: Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn tools() -> Value {
    json!([
        {
            "name": "bus_routes",
            "description": "Search Hong Kong bus routes across KMB/LWB, Citybus, green minibus, NLB and MTR Bus. Returns route number, direction, origin and destination.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "operator": {"type": "string", "enum": ["kmb", "ctb", "gmb", "nlb", "mtrbus"]},
                    "query": {"type": "string", "description": "Route number, origin or destination substring."},
                    "region": {"type": "string", "enum": ["HKI", "KLN", "NT"], "description": "Green minibus region only."},
                    "limit": {"type": "number", "description": "Maximum routes (default 40)."}
                },
                "required": ["operator"]
            }
        },
        {
            "name": "bus_stops",
            "description": "List the ordered stops and stop ids for a Hong Kong bus route.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "operator": {"type": "string", "enum": ["kmb", "ctb", "gmb", "nlb", "mtrbus"]},
                    "route": {"type": "string"},
                    "direction": {"type": "string", "description": "outbound/inbound, or O/I for KMB."}
                },
                "required": ["operator", "route"]
            }
        },
        {
            "name": "bus_eta",
            "description": "Live next-arrival predictions for a Hong Kong bus route at a stop, with minutes until arrival.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "operator": {"type": "string", "enum": ["kmb", "ctb", "gmb", "nlb", "mtrbus"]},
                    "route": {"type": "string"},
                    "stop": {"type": "string", "description": "Stop id or stop-name substring."},
                    "direction": {"type": "string", "description": "outbound/inbound, or O/I for KMB."}
                },
                "required": ["operator", "route", "stop"]
            }
        },
        {
            "name": "private_bus_eta",
            "description": "Live Park Island (PITCL) next-arrival predictions for NR330, NR332, NR334 and NR338, from the operator's public ETA backend. Discovery Bay has no public live feed.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "route": {"type": "string", "enum": ["NR330", "NR332", "NR334", "NR338"]},
                    "direction": {"type": "number", "enum": [1, 2], "description": "1 outbound, 2 inbound; omit for both."}
                },
                "required": ["route"]
            }
        },
        {
            "name": "private_bus_routes",
            "description": "Reference routes for private estate coaches without a live feed: Discovery Bay (DBTSL) and Park Island (PITCL).",
            "inputSchema": {
                "type": "object",
                "properties": {"query": {"type": "string"}}
            }
        },
        {
            "name": "red_minibus_routes",
            "description": "Red minibus routes published by 16seats.net, with district, service description and published route detail. Red minibuses remain non-scheduled with no live ETA.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "Route or place substring."},
                    "district": {"type": "string", "description": "District substring, e.g. Kowloon or Sha Tin."}
                }
            }
        },
        {
            "name": "red_minibus_route",
            "description": "Published timetable notes, fares and major boarding points for one red minibus route from 16seats.net. Fares and routeing remain at the driver's discretion.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "route": {"type": "string", "description": "Route code such as r_h08, or a route-name substring."}
                },
                "required": ["route"]
            }
        },
        {
            "name": "red_minibus_info",
            "description": "Reference notes and operating caveats for red minibuses.",
            "inputSchema": {"type": "object", "properties": {}}
        },
        {
            "name": "bus_stops_near",
            "description": "Stops within a radius of a WGS84 coordinate. KMB works without a route because it publishes a bulk stop list; Citybus and NLB need a route. GMB and MTR Bus do not publish stop coordinates.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "lat": {"type": "number", "description": "Latitude, e.g. 22.3193."},
                    "lng": {"type": "number", "description": "Longitude, e.g. 114.1694."},
                    "operator": {"type": "string", "enum": ["kmb", "ctb", "gmb", "nlb", "mtrbus"], "description": "Default kmb."},
                    "route": {"type": "string", "description": "Route number; required for ctb and nlb."},
                    "radius_km": {"type": "number", "minimum": 0.01, "maximum": 20, "description": "Search radius in kilometres. Default 0.5."},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 100, "description": "Default 20."}
                },
                "required": ["lat", "lng"]
            }
        },
        {
            "name": "bus_journey",
            "description": "Plan a direct or one-transfer bus journey between two stop or place names. Searches KMB, Citybus and NLB published route-stop data; green minibus route discovery is not covered yet.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "from": {"type": "string", "description": "Origin stop or place name, or a KMB stop id."},
                    "to": {"type": "string", "description": "Destination stop or place name, or a KMB stop id."},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 20, "description": "Default 5."}
                },
                "required": ["from", "to"]
            }
        }
    ])
}

async fn call(client: &Client, request: &Value) -> Result<String> {
    let params = request.get("params").cloned().unwrap_or(Value::Null);
    let name = params["name"].as_str().unwrap_or_default();
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let arg = |key: &str| args.get(key).and_then(Value::as_str).map(ToOwned::to_owned);
    match name {
        "bus_routes" => {
            let operator = Operator::parse(&arg("operator").unwrap_or_default())?;
            let limit = args
                .get("limit")
                .and_then(Value::as_u64)
                .unwrap_or(40)
                .max(1) as usize;
            let routes = client
                .routes(
                    operator,
                    arg("query").as_deref(),
                    arg("region").as_deref(),
                    limit,
                )
                .await?;
            Ok(render::routes(operator.label(), &routes))
        }
        "bus_stops" => {
            let operator = Operator::parse(&arg("operator").unwrap_or_default())?;
            let route = arg("route").ok_or_else(|| anyhow::anyhow!("route is required"))?;
            let stops = client
                .stops(operator, &route, arg("direction").as_deref())
                .await?;
            Ok(render::stops(&route, &stops))
        }
        "bus_eta" => {
            let operator = Operator::parse(&arg("operator").unwrap_or_default())?;
            let route = arg("route").ok_or_else(|| anyhow::anyhow!("route is required"))?;
            let stop = arg("stop").ok_or_else(|| anyhow::anyhow!("stop is required"))?;
            let mut etas = client
                .eta(operator, &route, &stop, arg("direction").as_deref())
                .await?;
            etas.sort_by_key(|eta| (eta.minutes.unwrap_or(i64::MAX), eta.eta.clone()));
            Ok(render::etas(&route, &stop, &etas))
        }
        "private_bus_routes" => Ok(render::private(arg("query").as_deref())),
        "private_bus_eta" => {
            let route = arg("route").ok_or_else(|| anyhow::anyhow!("route is required"))?;
            let direction = args
                .get("direction")
                .and_then(Value::as_u64)
                .map(|value| value as u8);
            let mut etas = client.pitcl_eta(&route, direction).await?;
            etas.sort_by_key(|eta| (eta.minutes.unwrap_or(i64::MAX), eta.eta.clone()));
            Ok(render::etas(&route, "Park Island stops", &etas))
        }
        "red_minibus_routes" => {
            let routes = client.red_routes().await?;
            let needle = arg("query").map(|value| value.to_lowercase());
            let area = arg("district").map(|value| value.to_lowercase());
            let filtered: Vec<_> = routes
                .into_iter()
                .filter(|route| {
                    needle.as_ref().is_none_or(|needle| {
                        route.name.to_lowercase().contains(needle)
                            || route.description.to_lowercase().contains(needle)
                    }) && area.as_ref().is_none_or(|area| {
                        route
                            .districts
                            .iter()
                            .any(|district| district.to_lowercase().contains(area))
                    })
                })
                .collect();
            Ok(render::red_routes(&filtered))
        }
        "red_minibus_route" => {
            let route = arg("route").ok_or_else(|| anyhow::anyhow!("route is required"))?;
            Ok(render::red_route(&client.red_route(&route).await?))
        }
        "red_minibus_info" => Ok(RED_MINIBUS.to_string()),
        "bus_stops_near" => {
            let lat = args
                .get("lat")
                .and_then(Value::as_f64)
                .ok_or_else(|| anyhow::anyhow!("lat is required"))?;
            let lng = args
                .get("lng")
                .and_then(Value::as_f64)
                .ok_or_else(|| anyhow::anyhow!("lng is required"))?;
            let operator = Operator::parse(arg("operator").as_deref().unwrap_or("kmb"))?;
            let radius = args.get("radius_km").and_then(Value::as_f64).unwrap_or(0.5);
            let limit = args
                .get("limit")
                .and_then(Value::as_u64)
                .unwrap_or(20)
                .max(1) as usize;
            let stops = client
                .stops_near(operator, lat, lng, radius, limit, arg("route").as_deref())
                .await?;
            Ok(serde_json::to_string_pretty(&stops)?)
        }
        "bus_journey" => {
            let from = arg("from").ok_or_else(|| anyhow::anyhow!("from is required"))?;
            let to = arg("to").ok_or_else(|| anyhow::anyhow!("to is required"))?;
            let limit = args
                .get("limit")
                .and_then(Value::as_u64)
                .unwrap_or(5)
                .max(1) as usize;
            let journeys = client.journey(&from, &to, limit).await?;
            Ok(serde_json::to_string_pretty(&json!({
                "from": from,
                "to": to,
                "count": journeys.len(),
                "journeys": journeys
            }))?)
        }
        other => anyhow::bail!("unknown tool: {other}"),
    }
}
