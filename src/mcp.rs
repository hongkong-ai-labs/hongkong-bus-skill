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
        let Ok(request) = serde_json::from_str::<Value>(line.trim()) else {
            continue;
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
            "name": "private_bus_routes",
            "description": "Reference routes for private estate coaches without a live feed: Discovery Bay (DBTSL) and Park Island (PITCL).",
            "inputSchema": {
                "type": "object",
                "properties": {"query": {"type": "string"}}
            }
        },
        {
            "name": "red_minibus_info",
            "description": "Explain why red minibuses have no fixed route or ETA data and how to use them.",
            "inputSchema": {"type": "object", "properties": {}}
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
        "red_minibus_info" => Ok(RED_MINIBUS.to_string()),
        other => anyhow::bail!("unknown tool: {other}"),
    }
}
