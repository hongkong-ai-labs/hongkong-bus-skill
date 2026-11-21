//! Human-readable bus tables.

use crate::api::{self, Eta, RedRoute, RedRouteDetail, RouteInfo, Stop};

pub fn routes(operator: &str, routes: &[RouteInfo]) -> String {
    let mut out = format!("{operator} routes\n\n");
    if routes.is_empty() {
        out.push_str("No routes matched.\n");
        return out;
    }
    out.push_str(&format!(
        "{:<8} {:<5} {:<4} {:<34} {:<34} {}\n",
        "Route", "Dir", "Type", "From", "To", "Note"
    ));
    out.push_str(&"-".repeat(112));
    out.push('\n');
    for route in routes {
        out.push_str(&format!(
            "{:<8} {:<5} {:<4} {:<34} {:<34} {}\n",
            route.route,
            route.direction,
            route.service_type,
            truncate(&route.origin, 33),
            truncate(&route.destination, 33),
            route.note
        ));
    }
    out
}

pub fn stops(route: &str, stops: &[Stop]) -> String {
    let mut out = format!("Stops for route {route}\n\n");
    for stop in stops {
        out.push_str(&format!(
            "{:<4} {:<14} {:<48} {} {}\n",
            stop.sequence,
            stop.id,
            truncate(&stop.name, 47),
            stop.direction,
            stop.note
        ));
    }
    out.push_str(&format!("\n{} stops.\n", stops.len()));
    out
}

pub fn etas(route: &str, stop: &str, etas: &[Eta]) -> String {
    let mut out = format!("Live arrivals — route {route} at {stop}\n\n");
    if etas.is_empty() {
        out.push_str("No arrivals are being reported. The route may not be running now, or the stop is at the end of its journey.\n");
        return out;
    }
    out.push_str(&format!(
        "{:<10} {:<8} {:<30} {:<10} {}\n",
        "Route", "Dir", "Destination", "Arrives", "Remark"
    ));
    out.push_str(&"-".repeat(86));
    out.push('\n');
    for eta in etas {
        let arrival = match eta.minutes {
            Some(minutes) => format!("{minutes} min"),
            None if eta.eta.is_empty() => "no ETA".to_string(),
            None => short_time(&eta.eta),
        };
        out.push_str(&format!(
            "{:<10} {:<8} {:<30} {:<10} {}\n",
            eta.route,
            eta.direction,
            truncate(&eta.destination, 29),
            arrival,
            eta.remark
        ));
    }
    out.push_str("\nLive predictions. Traffic can change them without notice.\n");
    out
}

pub fn red_routes(routes: &[RedRoute]) -> String {
    let mut out = format!(
        "Red minibus routes — {} from the 16seats.net directory\n\n",
        routes.len()
    );
    out.push_str(api::RED_MINIBUS);
    out.push('\n');
    out.push_str(&format!(
        "{:<10} {:<28} {:<34} {}\n",
        "Code", "District", "Route", "Service"
    ));
    out.push_str(&"-".repeat(120));
    out.push('\n');
    for route in routes {
        out.push_str(&format!(
            "{:<10} {:<28} {:<34} {}\n",
            route.code,
            truncate(&route.district, 27),
            truncate(&route.name, 33),
            truncate(&route.description, 48)
        ));
    }
    out.push_str(
        "\nUse `red-route <code or name>` for the published timetable, fares and stops.\n",
    );
    out
}

pub fn red_route(detail: &RedRouteDetail) -> String {
    format!(
        "{}\n{}\nSource: {}\n\n{}\n",
        detail.name, detail.district, detail.source, detail.detail
    )
}

pub fn private(query: Option<&str>) -> String {
    let mut out = String::from("Private bus and estate coach routes\n\n");
    out.push_str(api::private::PRIVATE_NOTE);
    out.push_str("\n\n");
    let needle = query.map(str::to_lowercase);
    for (operator, route, origin, destination, fare) in api::private::PRIVATE_ROUTES {
        if let Some(needle) = &needle
            && !operator.to_lowercase().contains(needle)
            && !route.to_lowercase().contains(needle)
            && !origin.to_lowercase().contains(needle)
            && !destination.to_lowercase().contains(needle)
        {
            continue;
        }
        out.push_str(&format!(
            "{:<24} {:<8} {:<24} → {:<30} {}\n",
            operator, route, origin, destination, fare
        ));
    }
    out.push_str(
        "\nDiscovery Bay: https://www.visitdiscoverybay.com/en/getting-around\nPark Island: http://www.pitcl.com.hk/en/bus_service.php\n",
    );
    out
}

fn short_time(value: &str) -> String {
    value
        .split_once('T')
        .map(|(_, time)| time.get(0..5).unwrap_or(time).to_string())
        .unwrap_or_else(|| value.to_string())
}

fn truncate(value: &str, width: usize) -> String {
    if value.chars().count() <= width {
        return value.to_string();
    }
    let mut out: String = value.chars().take(width.saturating_sub(1)).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn shortens_iso_times() {
        assert_eq!(super::short_time("2026-11-22T08:05:00+08:00"), "08:05");
        assert_eq!(
            super::short_time("Arriving / Departed"),
            "Arriving / Departed"
        );
    }
}
