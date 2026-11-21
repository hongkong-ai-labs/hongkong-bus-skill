//! `hkbus` — Hong Kong bus routes and live ETAs.

mod api;
mod mcp;
mod render;

use anyhow::{Result, bail};
use clap::{Parser, Subcommand};

use api::{Client, Operator};

#[derive(Parser)]
#[command(
    name = "hkbus",
    version,
    about = "Hong Kong bus routes and live ETAs: KMB, Citybus, green minibus, NLB, MTR Bus"
)]
struct Cli {
    /// Print machine-readable JSON.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Search routes, or list what an operator publishes.
    Routes {
        /// kmb, ctb, gmb, nlb or mtrbus.
        operator: String,
        /// Route number, origin or destination substring.
        #[arg(long)]
        query: Option<String>,
        /// GMB region: HKI, KLN or NT.
        #[arg(long)]
        region: Option<String>,
        /// Maximum routes to print.
        #[arg(long, default_value_t = 40)]
        limit: usize,
    },
    /// List the stops on a route, in order.
    Stops {
        /// kmb, ctb, gmb, nlb or mtrbus.
        operator: String,
        /// Route number, e.g. 1, 962 or K51.
        route: String,
        /// outbound/inbound (KMB also accepts O/I).
        #[arg(long)]
        direction: Option<String>,
    },
    /// Live next-arrival predictions for a route at a stop.
    Eta {
        /// kmb, ctb, gmb, nlb or mtrbus.
        operator: String,
        /// Route number.
        route: String,
        /// Stop id or stop-name substring.
        stop: String,
        /// outbound/inbound (KMB also accepts O/I).
        #[arg(long)]
        direction: Option<String>,
    },
    /// Private estate coaches with no live feed (Discovery Bay, Park Island).
    Private {
        /// Operator, route or place substring.
        #[arg(long)]
        query: Option<String>,
    },
    /// Red minibus routes published by 16seats.net.
    RedMinibus {
        /// Route or place substring.
        #[arg(long)]
        query: Option<String>,
        /// District substring, e.g. Kowloon or Sha Tin.
        #[arg(long)]
        district: Option<String>,
    },
    /// Published timetable, fares and major stops for one red minibus route.
    RedRoute {
        /// Route code (r_h08) or name substring.
        route: String,
    },
    /// Live Park Island (PITCL) arrivals for NR330/332/334/338.
    PrivateEta {
        /// PITCL route, e.g. NR330.
        route: String,
        /// Direction 1 or 2; omit for both.
        #[arg(long)]
        direction: Option<u8>,
    },
    /// Run the MCP server over stdio.
    Mcp,
}

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    let cli = Cli::parse();
    let client = Client::new()?;
    match cli.command {
        Command::Mcp => mcp::serve(client).await?,
        Command::RedMinibus { query, district } => {
            let routes = client.red_routes().await?;
            let needle = query.as_deref().map(str::to_lowercase);
            let area = district.as_deref().map(str::to_lowercase);
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
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&filtered)?);
            } else {
                print!("{}", render::red_routes(&filtered));
            }
        }
        Command::PrivateEta { route, direction } => {
            let mut etas = client.pitcl_eta(&route, direction).await?;
            etas.sort_by_key(|eta| (eta.minutes.unwrap_or(i64::MAX), eta.eta.clone()));
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&etas)?);
            } else {
                print!("{}", render::etas(&route, "Park Island stops", &etas));
            }
        }
        Command::RedRoute { route } => {
            let detail = client.red_route(&route).await?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&detail)?);
            } else {
                print!("{}", render::red_route(&detail));
            }
        }
        Command::Private { query } => {
            if cli.json {
                let needle = query.as_deref().map(str::to_lowercase);
                let rows: Vec<_> = api::private::PRIVATE_ROUTES
                    .iter()
                    .filter(|(operator, route, origin, destination, _)| {
                        needle.as_ref().is_none_or(|needle| {
                            operator.to_lowercase().contains(needle)
                                || route.to_lowercase().contains(needle)
                                || origin.to_lowercase().contains(needle)
                                || destination.to_lowercase().contains(needle)
                        })
                    })
                    .map(|(operator, route, origin, destination, fare)| {
                        serde_json::json!({
                            "operator": operator,
                            "route": route,
                            "origin": origin,
                            "destination": destination,
                            "fare": fare
                        })
                    })
                    .collect();
                println!("{}", serde_json::to_string_pretty(&rows)?);
            } else {
                print!("{}", render::private(query.as_deref()));
            }
        }
        Command::Routes {
            operator,
            query,
            region,
            limit,
        } => {
            let operator = Operator::parse(&operator)?;
            if limit == 0 {
                bail!("--limit must be at least 1");
            }
            let routes = client
                .routes(operator, query.as_deref(), region.as_deref(), limit)
                .await?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&routes)?);
            } else {
                print!("{}", render::routes(operator.label(), &routes));
            }
        }
        Command::Stops {
            operator,
            route,
            direction,
        } => {
            let operator = Operator::parse(&operator)?;
            let stops = client.stops(operator, &route, direction.as_deref()).await?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&stops)?);
            } else {
                print!("{}", render::stops(&route, &stops));
            }
        }
        Command::Eta {
            operator,
            route,
            stop,
            direction,
        } => {
            let operator = Operator::parse(&operator)?;
            let mut etas = client
                .eta(operator, &route, &stop, direction.as_deref())
                .await?;
            etas.sort_by_key(|eta| (eta.minutes.unwrap_or(i64::MAX), eta.eta.clone()));
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&etas)?);
            } else {
                print!("{}", render::etas(&route, &stop, &etas));
            }
        }
    }
    Ok(())
}
