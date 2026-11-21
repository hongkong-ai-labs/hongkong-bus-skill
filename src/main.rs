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
    /// Why red minibuses have no route or ETA data.
    RedMinibus,
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
        Command::RedMinibus => print!("{}", api::RED_MINIBUS),
        Command::Private { query } => {
            if cli.json {
                let needle = query.as_deref().map(str::to_lowercase);
                let rows: Vec<_> = api::private::PRIVATE_ROUTES
                    .iter()
                    .filter(|(operator, route, origin, destination)| {
                        needle.as_ref().is_none_or(|needle| {
                            operator.to_lowercase().contains(needle)
                                || route.to_lowercase().contains(needle)
                                || origin.to_lowercase().contains(needle)
                                || destination.to_lowercase().contains(needle)
                        })
                    })
                    .map(|(operator, route, origin, destination)| {
                        serde_json::json!({
                            "operator": operator,
                            "route": route,
                            "origin": origin,
                            "destination": destination
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
