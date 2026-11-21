//! Hong Kong bus open-data clients.
//!
//! Live feeds: KMB/LWB, Citybus, green minibus (GMB), New Lantao Bus (NLB)
//! and MTR Bus. Red minibuses and private estate coaches have no fixed route
//! or ETA feed; they are represented by reference data in [`private`].

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde_json::Value;

const KMB: &str = "https://data.etabus.gov.hk/v1/transport/kmb";
const CTB: &str = "https://rt.data.gov.hk/v2/transport/citybus";
const GMB: &str = "https://data.etagmb.gov.hk";
const NLB: &str = "https://rt.data.gov.hk/v2/transport/nlb";
const MTR_BUS: &str = "https://rt.data.gov.hk/v1/transport/mtr/bus/getSchedule";
const SIXTEEN_SEATS: &str = "https://www.16seats.net/eng/rmb";
const PITCL_API: &str = "https://360-api.socif.co";

/// Park Island routes with live ETA on the Socif backend.
pub const PITCL_LIVE: [(&str, u64); 4] = [
    ("NR330", 249),
    ("NR332", 250),
    ("NR334", 252),
    ("NR338", 253),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operator {
    Kmb,
    Ctb,
    Gmb,
    Nlb,
    MtrBus,
}

impl Operator {
    pub fn parse(value: &str) -> Result<Self> {
        match value.to_lowercase().replace(['-', '_'], "").as_str() {
            "kmb" | "lwb" => Ok(Self::Kmb),
            "ctb" | "citybus" => Ok(Self::Ctb),
            "gmb" | "minibus" | "greenminibus" => Ok(Self::Gmb),
            "nlb" | "lantao" => Ok(Self::Nlb),
            "mtr" | "mtrbus" => Ok(Self::MtrBus),
            other => bail!("unknown operator {other:?}; use kmb, ctb, gmb, nlb or mtrbus"),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Kmb => "KMB / LWB",
            Self::Ctb => "Citybus",
            Self::Gmb => "Green minibus",
            Self::Nlb => "New Lantao Bus",
            Self::MtrBus => "MTR Bus",
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Self::Kmb => "kmb",
            Self::Ctb => "ctb",
            Self::Gmb => "gmb",
            Self::Nlb => "nlb",
            Self::MtrBus => "mtrbus",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RouteInfo {
    pub operator: String,
    pub route: String,
    /// O (outbound) or I (inbound) when the operator publishes one.
    pub direction: String,
    pub service_type: String,
    pub origin: String,
    pub destination: String,
    /// Region for GMB, route id for NLB, otherwise empty.
    pub note: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Stop {
    pub id: String,
    pub name: String,
    pub sequence: i64,
    pub direction: String,
    pub service_type: String,
    pub note: String,
    /// WGS84 latitude when the operator publishes coordinates.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lat: Option<f64>,
    /// WGS84 longitude when the operator publishes coordinates.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lng: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RedRoute {
    /// 16seats route code, e.g. `r_h08`.
    pub code: String,
    /// The first directory page that listed the route.
    pub district: String,
    /// Every district page that lists the route.
    pub districts: Vec<String>,
    pub name: String,
    pub description: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RedRouteDetail {
    pub code: String,
    pub name: String,
    pub district: String,
    /// Cleaned official route page: timetable, fares and major stops.
    pub detail: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct NearbyStop {
    pub operator: String,
    pub id: String,
    pub name: String,
    pub distance_km: f64,
    pub lat: f64,
    pub lng: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct JourneyLeg {
    pub operator: String,
    pub route: String,
    pub direction: String,
    pub service_type: String,
    pub board: String,
    pub board_id: String,
    pub alight: String,
    pub alight_id: String,
    /// Stops travelled on this leg.
    pub stops: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Journey {
    pub from: String,
    pub to: String,
    pub kind: String,
    pub transfers: usize,
    pub total_stops: i64,
    pub legs: Vec<JourneyLeg>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Eta {
    pub operator: String,
    pub route: String,
    pub direction: String,
    pub destination: String,
    pub stop: String,
    pub eta: String,
    pub minutes: Option<i64>,
    pub remark: String,
    pub data_time: String,
}

pub struct Client {
    http: reqwest::Client,
    cache: Mutex<HashMap<String, (Instant, Value)>>,
    html_cache: Mutex<HashMap<String, (Instant, String)>>,
}

impl Client {
    pub fn new() -> Result<Self> {
        Ok(Self {
            http: reqwest::Client::builder()
                .user_agent(concat!(
                    "hongkong-bus-skill/",
                    env!("CARGO_PKG_VERSION"),
                    " (+https://github.com/hongkong-ai-labs/hongkong-bus-skill)"
                ))
                .timeout(Duration::from_secs(30))
                .build()?,
            cache: Mutex::new(HashMap::new()),
            html_cache: Mutex::new(HashMap::new()),
        })
    }

    async fn get(&self, url: &str, ttl: Duration) -> Result<Value> {
        if let Some((at, value)) = self
            .cache
            .lock()
            .expect("bus cache poisoned")
            .get(url)
            .cloned()
            && at.elapsed() < ttl
        {
            return Ok(value);
        }
        let response = self
            .http
            .get(url)
            .send()
            .await
            .with_context(|| format!("GET {url}"))?;
        let status = response.status();
        let body = response.text().await?;
        if !status.is_success() {
            bail!("{url} returned HTTP {status}");
        }
        let value: Value =
            serde_json::from_str(&body).with_context(|| format!("decoding {url}"))?;
        self.cache
            .lock()
            .expect("bus cache poisoned")
            .insert(url.to_string(), (Instant::now(), value.clone()));
        Ok(value)
    }

    async fn get_html(&self, url: &str) -> Result<String> {
        if let Some((at, body)) = self
            .html_cache
            .lock()
            .expect("bus cache poisoned")
            .get(url)
            .cloned()
            && at.elapsed() < Duration::from_secs(21_600)
        {
            return Ok(body);
        }
        let response = self
            .http
            .get(url)
            .send()
            .await
            .with_context(|| format!("GET {url}"))?;
        let status = response.status();
        let body = response.text().await?;
        if !status.is_success() {
            bail!("{url} returned HTTP {status}");
        }
        self.html_cache
            .lock()
            .expect("bus cache poisoned")
            .insert(url.to_string(), (Instant::now(), body.clone()));
        Ok(body)
    }

    /// Red minibus routes from the 16seats.net public route directory.
    pub async fn red_routes(&self) -> Result<Vec<RedRoute>> {
        let mut routes = Vec::new();
        for (slug, district) in RED_DISTRICTS {
            let url = format!("{SIXTEEN_SEATS}/r_{slug}.html");
            let html = self.get_html(&url).await?;
            parse_red_list(&html, district, &url, &mut routes)?;
        }
        let mut routes = dedupe_red_routes(routes);
        routes.sort_by(|left, right| {
            left.district
                .cmp(&right.district)
                .then(left.name.cmp(&right.name))
        });
        Ok(routes)
    }

    /// Full published page for one red minibus route.
    pub async fn red_route(&self, query: &str) -> Result<RedRouteDetail> {
        let routes = self.red_routes().await?;
        let needle = query.to_lowercase();
        let mut matches: Vec<&RedRoute> = routes
            .iter()
            .filter(|route| {
                route.code.eq_ignore_ascii_case(query)
                    || route
                        .code
                        .trim_start_matches("r_")
                        .eq_ignore_ascii_case(query)
                    || route.name.to_lowercase().contains(&needle)
            })
            .collect();
        matches.sort_by_key(|route| {
            (
                !route.name.to_lowercase().starts_with(&needle),
                route.name.len(),
            )
        });
        let route = match matches.len() {
            0 => bail!("no red minibus route matches {query:?}"),
            1 => matches[0],
            _ => {
                let options: Vec<String> = matches
                    .iter()
                    .take(10)
                    .map(|route| format!("{} ({})", route.name, route.code))
                    .collect();
                bail!(
                    "{query:?} matches several red minibus routes: {}. Use a route code.",
                    options.join(", ")
                );
            }
        };
        let html = self.get_html(&route.url).await?;
        Ok(RedRouteDetail {
            code: route.code.clone(),
            name: route.name.clone(),
            district: route.districts.join(" / "),
            detail: clean_route_page(&html),
            source: route.url.clone(),
        })
    }

    /// Live Park Island (PITCL) arrivals for NR330/332/334/338.
    pub async fn pitcl_eta(&self, route: &str, direction: Option<u8>) -> Result<Vec<Eta>> {
        let (code, id) = PITCL_LIVE
            .iter()
            .find(|(code, _)| code.eq_ignore_ascii_case(route))
            .map(|(code, id)| (*code, *id))
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "no live Park Island ETA for {route}; use NR330, NR332, NR334 or NR338"
                )
            })?;
        let meta = self
            .get(
                &format!("{PITCL_API}/api/route/{id}"),
                Duration::from_secs(3600),
            )
            .await?;
        let mut labels = HashMap::new();
        for row in meta["data"].as_array().into_iter().flatten() {
            for direction in row["directions"].as_array().into_iter().flatten() {
                if let (Some(seq), Some(orig), Some(dest)) = (
                    direction["route_seq"].as_u64(),
                    direction["orig_en"].as_str(),
                    direction["dest_en"].as_str(),
                ) {
                    labels.insert(seq as u8, format!("{orig} → {dest}"));
                }
            }
        }
        let sequences: Vec<u8> = match direction {
            Some(seq @ (1 | 2)) => vec![seq],
            Some(other) => bail!("direction must be 1 or 2, got {other}"),
            None => vec![1, 2],
        };
        let mut out = Vec::new();
        for seq in sequences {
            let stops = self
                .get(
                    &format!("{PITCL_API}/api/route-stop/{id}/{seq}"),
                    Duration::from_secs(3600),
                )
                .await?;
            let names: HashMap<i64, String> = stops["data"]["route_stops"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|row| {
                    Some((
                        row["stop_seq"].as_i64()?,
                        row["name_en"].as_str()?.to_string(),
                    ))
                })
                .collect();
            let etas = self
                .get(
                    &format!("{PITCL_API}/api/eta/route-stop/{id}/{seq}"),
                    Duration::from_secs(30),
                )
                .await?;
            for stop in etas["data"]["eta"].as_array().into_iter().flatten() {
                let Some(stop_seq) = stop["stopSeq"].as_i64() else {
                    continue;
                };
                let Some(stop_name) = names.get(&stop_seq) else {
                    continue;
                };
                for eta in stop["eta"].as_array().into_iter().flatten() {
                    out.push(Eta {
                        operator: "pitcl".into(),
                        route: code.into(),
                        direction: seq.to_string(),
                        destination: labels.get(&seq).cloned().unwrap_or_default(),
                        stop: stop_name.clone(),
                        eta: text(eta, "timestamp"),
                        minutes: eta["diff"].as_i64(),
                        remark: text(eta, "remarks_en"),
                        data_time: text(&etas, "generated_timestamp"),
                    });
                }
            }
        }
        Ok(out)
    }

    async fn post(&self, url: &str, body: Value) -> Result<Value> {
        let response = self
            .http
            .post(url)
            .json(&body)
            .send()
            .await
            .with_context(|| format!("POST {url}"))?;
        let status = response.status();
        let text = response.text().await?;
        if !status.is_success() {
            bail!("{url} returned HTTP {status}");
        }
        serde_json::from_str(&text).with_context(|| format!("decoding {url}"))
    }

    pub async fn routes(
        &self,
        operator: Operator,
        query: Option<&str>,
        region: Option<&str>,
        limit: usize,
    ) -> Result<Vec<RouteInfo>> {
        let needle = query.map(str::to_lowercase);
        let matches = |route: &str, origin: &str, destination: &str| {
            needle.as_ref().is_none_or(|needle| {
                route.to_lowercase().contains(needle)
                    || origin.to_lowercase().contains(needle)
                    || destination.to_lowercase().contains(needle)
            })
        };
        let mut out = Vec::new();
        match operator {
            Operator::Kmb => {
                let value = self
                    .get(&format!("{KMB}/route/"), Duration::from_secs(3600))
                    .await?;
                for row in value["data"].as_array().into_iter().flatten() {
                    let route = text(row, "route");
                    let origin = text(row, "orig_en");
                    let destination = text(row, "dest_en");
                    if !matches(&route, &origin, &destination) {
                        continue;
                    }
                    out.push(RouteInfo {
                        operator: "kmb".into(),
                        route,
                        direction: text(row, "bound"),
                        service_type: text(row, "service_type"),
                        origin,
                        destination,
                        note: String::new(),
                    });
                }
            }
            Operator::Ctb => {
                let value = self
                    .get(&format!("{CTB}/route/CTB"), Duration::from_secs(3600))
                    .await?;
                for row in value["data"].as_array().into_iter().flatten() {
                    let route = text(row, "route");
                    let origin = text(row, "orig_en");
                    let destination = text(row, "dest_en");
                    if !matches(&route, &origin, &destination) {
                        continue;
                    }
                    out.push(RouteInfo {
                        operator: "ctb".into(),
                        route,
                        direction: String::new(),
                        service_type: String::new(),
                        origin,
                        destination,
                        note: String::new(),
                    });
                }
            }
            Operator::Gmb => {
                let value = self
                    .get(&format!("{GMB}/route/"), Duration::from_secs(3600))
                    .await?;
                let regions = value["data"]["routes"]
                    .as_object()
                    .context("GMB route list changed shape")?;
                for (code, routes) in regions {
                    if let Some(wanted) = region
                        && !code.eq_ignore_ascii_case(wanted)
                    {
                        continue;
                    }
                    for route in routes.as_array().into_iter().flatten() {
                        let route = route.as_str().unwrap_or_default();
                        if !matches(route, "", "") {
                            continue;
                        }
                        out.push(RouteInfo {
                            operator: "gmb".into(),
                            route: route.to_string(),
                            direction: String::new(),
                            service_type: String::new(),
                            origin: String::new(),
                            destination: String::new(),
                            note: code.clone(),
                        });
                    }
                }
            }
            Operator::Nlb => {
                let value = self
                    .get(
                        &format!("{NLB}/route.php?action=list"),
                        Duration::from_secs(3600),
                    )
                    .await?;
                for row in value["routes"].as_array().into_iter().flatten() {
                    let route = text(row, "routeNo");
                    let name = text(row, "routeName_e");
                    if !matches(&route, &name, "") {
                        continue;
                    }
                    let (origin, destination) = name
                        .split_once(" > ")
                        .map(|(left, right)| (left.to_string(), right.to_string()))
                        .unwrap_or_else(|| (name.clone(), String::new()));
                    out.push(RouteInfo {
                        operator: "nlb".into(),
                        route,
                        direction: String::new(),
                        service_type: String::new(),
                        origin,
                        destination,
                        note: row["routeId"].to_string(),
                    });
                }
            }
            Operator::MtrBus => {
                for (route, origin, destination) in private::MTR_BUS_ROUTES {
                    if matches(route, origin, destination) {
                        out.push(RouteInfo {
                            operator: "mtrbus".into(),
                            route: (*route).into(),
                            direction: String::new(),
                            service_type: String::new(),
                            origin: (*origin).into(),
                            destination: (*destination).into(),
                            note: String::new(),
                        });
                    }
                }
            }
        }
        out.sort_by(|left, right| {
            left.route
                .cmp(&right.route)
                .then(left.direction.cmp(&right.direction))
                .then(left.service_type.cmp(&right.service_type))
        });
        out.dedup_by(|left, right| {
            left.route == right.route
                && left.direction == right.direction
                && left.service_type == right.service_type
        });
        out.truncate(limit);
        Ok(out)
    }

    pub async fn stops(
        &self,
        operator: Operator,
        route: &str,
        direction: Option<&str>,
    ) -> Result<Vec<Stop>> {
        let raw = direction.map(str::to_string);
        let direction = raw.as_deref().map(kmb_bound);
        match operator {
            Operator::Kmb => {
                let variants = self.kmb_variants(route, direction.as_deref()).await?;
                let mut out = Vec::new();
                for (bound, service_type) in variants {
                    let value = self
                        .get(
                            &format!(
                                "{KMB}/route-stop/{route}/{}/{service_type}",
                                kmb_word(&bound)
                            ),
                            Duration::from_secs(3600),
                        )
                        .await?;
                    for row in value["data"].as_array().into_iter().flatten() {
                        let id = text(row, "stop");
                        let (name, lat, lng) = self.kmb_stop(&id).await?;
                        out.push(Stop {
                            id,
                            name,
                            sequence: row["seq"]
                                .as_str()
                                .and_then(|v| v.parse().ok())
                                .unwrap_or(0),
                            direction: bound.clone(),
                            service_type: service_type.clone(),
                            note: String::new(),
                            lat,
                            lng,
                        });
                    }
                }
                Ok(out)
            }
            Operator::Ctb => {
                let directions = match raw.as_deref() {
                    Some(value) => vec![ctb_direction(value)],
                    None => vec!["outbound".to_string(), "inbound".to_string()],
                };
                let mut out = Vec::new();
                for direction in directions {
                    let value = self
                        .get(
                            &format!("{CTB}/route-stop/CTB/{route}/{direction}"),
                            Duration::from_secs(3600),
                        )
                        .await?;
                    for row in value["data"].as_array().into_iter().flatten() {
                        let id = text(row, "stop");
                        let (name, lat, lng) = self.ctb_stop(&id).await?;
                        out.push(Stop {
                            id,
                            name,
                            sequence: row["seq"].as_i64().unwrap_or(0),
                            direction: direction.clone(),
                            service_type: String::new(),
                            note: String::new(),
                            lat,
                            lng,
                        });
                    }
                }
                Ok(out)
            }
            Operator::Gmb => {
                let (region, route_id, variants) = self.gmb_variants(route).await?;
                let wanted: Vec<(i64, String)> = match raw.as_deref() {
                    Some(value) => variants
                        .into_iter()
                        .filter(|(seq, _)| gmb_direction(*seq, value))
                        .collect(),
                    None => variants,
                };
                if wanted.is_empty() {
                    bail!("GMB route {route} ({region}) has no direction {direction:?}");
                }
                let mut out = Vec::new();
                for (sequence, label) in wanted {
                    let value = self
                        .get(
                            &format!("{GMB}/route-stop/{route_id}/{sequence}"),
                            Duration::from_secs(3600),
                        )
                        .await?;
                    for row in value["data"]["route_stops"]
                        .as_array()
                        .into_iter()
                        .flatten()
                    {
                        out.push(Stop {
                            id: row["stop_id"].to_string(),
                            name: text(row, "name_en"),
                            sequence: row["stop_seq"].as_i64().unwrap_or(0),
                            direction: sequence.to_string(),
                            service_type: String::new(),
                            note: label.clone(),
                            lat: None,
                            lng: None,
                        });
                    }
                }
                Ok(out)
            }
            Operator::Nlb => {
                let route_id = self.nlb_route_id(route).await?;
                let value = self
                    .get(
                        &format!("{NLB}/stop.php?action=list&routeId={route_id}"),
                        Duration::from_secs(3600),
                    )
                    .await?;
                Ok(value["stops"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .enumerate()
                    .map(|(index, row)| Stop {
                        id: row["stopId"].to_string(),
                        name: text(row, "stopName_e"),
                        sequence: index as i64 + 1,
                        direction: String::new(),
                        service_type: String::new(),
                        note: format!("fare {}", text(row, "fare")),
                        lat: number(row, "latitude"),
                        lng: number(row, "longitude"),
                    })
                    .collect())
            }
            Operator::MtrBus => {
                let value = self.mtr_schedule(route).await?;
                Ok(value["busStop"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .enumerate()
                    .map(|(index, row)| Stop {
                        id: text(row, "busStopId"),
                        name: text(row, "busStopId"),
                        sequence: index as i64 + 1,
                        direction: String::new(),
                        service_type: String::new(),
                        note: "MTR Bus publishes stop ids only".into(),
                        lat: None,
                        lng: None,
                    })
                    .collect())
            }
        }
    }

    /// Stops within `radius_km` of a coordinate.
    ///
    /// KMB publishes a bulk stop list, so it needs no route. Citybus and NLB
    /// carry coordinates in their route stop lists and therefore need
    /// `route`. GMB and MTR Bus do not publish stop coordinates.
    pub async fn stops_near(
        &self,
        operator: Operator,
        lat: f64,
        lng: f64,
        radius_km: f64,
        limit: usize,
        route: Option<&str>,
    ) -> Result<Vec<NearbyStop>> {
        if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lng) {
            bail!("lat must be -90..90 and lng must be -180..180");
        }
        if !(radius_km.is_finite() && radius_km > 0.0) {
            bail!("radius must be a positive number of kilometres");
        }
        let radius_km = radius_km.min(20.0);
        let limit = limit.clamp(1, 100);

        let stops = match route {
            Some(route) => self.stops(operator, route, None).await?,
            None if operator == Operator::Kmb => self.kmb_all_stops().await?,
            None => bail!(
                "{} does not publish a bulk stop list; pass --route to search one route",
                operator.label()
            ),
        };

        let mut with_coordinates = 0usize;
        let mut out: Vec<NearbyStop> = stops
            .into_iter()
            .filter_map(|stop| {
                let (stop_lat, stop_lng) = (stop.lat?, stop.lng?);
                with_coordinates += 1;
                let distance = haversine_km(lat, lng, stop_lat, stop_lng);
                (distance <= radius_km).then(|| NearbyStop {
                    operator: operator.id().to_string(),
                    id: stop.id,
                    name: stop.name,
                    distance_km: (distance * 100.0).round() / 100.0,
                    lat: stop_lat,
                    lng: stop_lng,
                })
            })
            .collect();

        if with_coordinates == 0 {
            bail!(
                "{} stop data for this route has no coordinates; nearby search works for KMB (all stops), Citybus and NLB (with --route)",
                operator.label()
            );
        }
        out.sort_by(|left, right| {
            left.distance_km
                .partial_cmp(&right.distance_km)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        out.dedup_by(|left, right| left.id == right.id);
        out.truncate(limit);
        if out.is_empty() {
            bail!("no stops within {radius_km} km; widen the radius");
        }
        Ok(out)
    }

    /// Best-effort direct and one-transfer journeys across KMB, Citybus and
    /// NLB. Green minibus route discovery is not covered yet because its feed
    /// has no bulk route-stop endpoint.
    pub async fn journey(&self, from: &str, to: &str, limit: usize) -> Result<Vec<Journey>> {
        let from = from.trim();
        let to = to.trim();
        if from.is_empty() || to.is_empty() {
            bail!("from and to must not be empty");
        }
        if from.eq_ignore_ascii_case(to) {
            bail!("from and to are the same place");
        }
        let limit = limit.clamp(1, 20);

        let index = self.kmb_index().await?;
        let from_stops = matching_stops(&index, from);
        let to_stops = matching_stops(&index, to);
        if from_stops.is_empty() {
            bail!("no KMB stop matches {from:?}");
        }
        if to_stops.is_empty() {
            bail!("no KMB stop matches {to:?}");
        }

        let mut out = direct_journeys(&index, &from_stops, &to_stops, from, to);
        if out.is_empty() {
            out = transfer_journeys(&index, &from_stops, &to_stops, from, to, limit);
        }
        // Citybus stop details are a per-stop request, so only ask the other
        // operators when KMB found nothing. This keeps the common case fast.
        if out.is_empty() {
            out.extend(self.candidate_direct(from, to).await?);
        }

        out.sort_by(|left, right| {
            left.transfers
                .cmp(&right.transfers)
                .then(left.total_stops.cmp(&right.total_stops))
        });
        out.dedup_by(|left, right| {
            left.legs.len() == right.legs.len()
                && left.legs.iter().zip(&right.legs).all(|(a, b)| {
                    a.route == b.route && a.board_id == b.board_id && a.alight_id == b.alight_id
                })
        });
        out.truncate(limit);
        if out.is_empty() {
            bail!(
                "no direct KMB, Citybus or NLB route connects {from:?} and {to:?}; try a nearby stop name or a route number"
            );
        }
        Ok(out)
    }

    async fn kmb_all_stops(&self) -> Result<Vec<Stop>> {
        let value = self
            .get(&format!("{KMB}/stop/"), Duration::from_secs(86_400))
            .await?;
        Ok(value["data"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|row| {
                let id = text(row, "stop");
                let name = text(row, "name_en");
                let lat = number(row, "lat")?;
                let lng = number(row, "long")?;
                if id.is_empty() || name.is_empty() {
                    return None;
                }
                Some(Stop {
                    id,
                    name,
                    sequence: 0,
                    direction: String::new(),
                    service_type: String::new(),
                    note: String::new(),
                    lat: Some(lat),
                    lng: Some(lng),
                })
            })
            .collect())
    }

    async fn kmb_index(&self) -> Result<KmbIndex> {
        let stops_value = self
            .get(&format!("{KMB}/stop/"), Duration::from_secs(86_400))
            .await?;
        let mut stops = HashMap::new();
        for row in stops_value["data"].as_array().into_iter().flatten() {
            let id = text(row, "stop");
            let name = text(row, "name_en");
            let (Some(lat), Some(lng)) = (number(row, "lat"), number(row, "long")) else {
                continue;
            };
            if !id.is_empty() && !name.is_empty() {
                stops.insert(id, (name, lat, lng));
            }
        }

        let routes_value = self
            .get(&format!("{KMB}/route-stop/"), Duration::from_secs(3600))
            .await?;
        let mut routes: HashMap<(String, String, String), Vec<(i64, String)>> = HashMap::new();
        let mut by_stop: HashMap<String, Vec<(String, String, String, i64)>> = HashMap::new();
        for row in routes_value["data"].as_array().into_iter().flatten() {
            let route = text(row, "route");
            let bound = text(row, "bound");
            let service_type = text(row, "service_type");
            let stop = text(row, "stop");
            let sequence = row["seq"]
                .as_i64()
                .or_else(|| text(row, "seq").parse().ok())
                .unwrap_or_default();
            if route.is_empty() || stop.is_empty() || sequence == 0 {
                continue;
            }
            routes
                .entry((route.clone(), bound.clone(), service_type.clone()))
                .or_default()
                .push((sequence, stop.clone()));
            by_stop
                .entry(stop)
                .or_default()
                .push((route, bound, service_type, sequence));
        }
        for stops_on_route in routes.values_mut() {
            stops_on_route.sort_by_key(|(sequence, _)| *sequence);
        }
        Ok(KmbIndex {
            stops,
            routes,
            by_stop,
        })
    }

    async fn candidate_direct(&self, from: &str, to: &str) -> Result<Vec<Journey>> {
        let mut candidates: Vec<(Operator, RouteInfo)> = Vec::new();
        for operator in [Operator::Ctb, Operator::Nlb] {
            for query in [from, to] {
                if let Ok(routes) = self.routes(operator, Some(query), None, 30).await {
                    candidates.extend(routes.into_iter().map(|info| (operator, info)));
                }
            }
        }
        candidates.sort_by(|(left_operator, left), (right_operator, right)| {
            left_operator
                .id()
                .cmp(right_operator.id())
                .then(left.route.cmp(&right.route))
                .then(left.direction.cmp(&right.direction))
        });
        candidates.dedup_by(|(left_operator, left), (right_operator, right)| {
            left_operator.id() == right_operator.id()
                && left.route == right.route
                && left.direction == right.direction
                && left.service_type == right.service_type
        });
        candidates.truncate(8);

        let mut out = Vec::new();
        for (operator, info) in candidates {
            let stops = match self.stops(operator, &info.route, None).await {
                Ok(stops) => stops,
                Err(_) => continue,
            };
            let mut groups: HashMap<(String, String), Vec<Stop>> = HashMap::new();
            for stop in stops {
                groups
                    .entry((stop.direction.clone(), stop.service_type.clone()))
                    .or_default()
                    .push(stop);
            }
            for group in groups.values() {
                let board = group
                    .iter()
                    .find(|stop| stop.id == from || stop_matches(&stop.name, from));
                let alight = group
                    .iter()
                    .find(|stop| stop.id == to || stop_matches(&stop.name, to));
                let (Some(board), Some(alight)) = (board, alight) else {
                    continue;
                };
                if alight.sequence <= board.sequence {
                    continue;
                }
                let total = alight.sequence - board.sequence;
                out.push(Journey {
                    from: from.to_string(),
                    to: to.to_string(),
                    kind: "direct".into(),
                    transfers: 0,
                    total_stops: total,
                    legs: vec![JourneyLeg {
                        operator: operator.id().to_string(),
                        route: info.route.clone(),
                        direction: direction_label(&board.direction),
                        service_type: board.service_type.clone(),
                        board: board.name.clone(),
                        board_id: board.id.clone(),
                        alight: alight.name.clone(),
                        alight_id: alight.id.clone(),
                        stops: total,
                    }],
                });
            }
        }
        Ok(out)
    }

    pub async fn eta(
        &self,
        operator: Operator,
        route: &str,
        stop_query: &str,
        direction: Option<&str>,
    ) -> Result<Vec<Eta>> {
        let raw = direction.map(str::to_string);
        let direction = raw.as_deref().map(kmb_bound);
        match operator {
            Operator::Kmb => {
                let variants = self.kmb_variants(route, direction.as_deref()).await?;
                let mut out = Vec::new();
                for (bound, service_type) in variants {
                    let Some((stop_id, stop_name)) = self
                        .kmb_find_stop(route, &bound, &service_type, stop_query)
                        .await?
                    else {
                        continue;
                    };
                    let value = self
                        .get(
                            &format!("{KMB}/eta/{stop_id}/{route}/{service_type}"),
                            Duration::from_secs(30),
                        )
                        .await?;
                    for row in value["data"].as_array().into_iter().flatten() {
                        if text(row, "dir") != bound {
                            continue;
                        }
                        let eta = text(row, "eta");
                        out.push(Eta {
                            operator: "kmb".into(),
                            route: route.into(),
                            direction: bound.clone(),
                            destination: text(row, "dest_en"),
                            stop: stop_name.clone(),
                            minutes: minutes_until(&eta),
                            eta,
                            remark: text(row, "rmk_en"),
                            data_time: text(row, "data_timestamp"),
                        });
                    }
                }
                Ok(out)
            }
            Operator::Ctb => {
                let directions = match raw.as_deref() {
                    Some(value) => vec![ctb_direction(value)],
                    None => vec!["outbound".to_string(), "inbound".to_string()],
                };
                let mut out = Vec::new();
                for direction in directions {
                    let Some((stop_id, stop_name)) =
                        self.ctb_find_stop(route, &direction, stop_query).await?
                    else {
                        continue;
                    };
                    let value = self
                        .get(
                            &format!("{CTB}/eta/CTB/{stop_id}/{route}"),
                            Duration::from_secs(30),
                        )
                        .await?;
                    for row in value["data"].as_array().into_iter().flatten() {
                        let eta = text(row, "eta");
                        out.push(Eta {
                            operator: "ctb".into(),
                            route: route.into(),
                            direction: direction.clone(),
                            destination: text(row, "dest_en"),
                            stop: stop_name.clone(),
                            minutes: minutes_until(&eta),
                            eta,
                            remark: text(row, "rmk_en"),
                            data_time: text(row, "data_timestamp"),
                        });
                    }
                }
                Ok(out)
            }
            Operator::Gmb => {
                let (_region, route_id, variants) = self.gmb_variants(route).await?;
                let wanted: Vec<(i64, String)> = match raw.as_deref() {
                    Some(value) => variants
                        .into_iter()
                        .filter(|(seq, _)| gmb_direction(*seq, value))
                        .collect(),
                    None => variants,
                };
                let mut out = Vec::new();
                for (sequence, label) in wanted {
                    let value = self
                        .get(
                            &format!("{GMB}/route-stop/{route_id}/{sequence}"),
                            Duration::from_secs(3600),
                        )
                        .await?;
                    let Some(row) = value["data"]["route_stops"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .find(|row| {
                            let id = row["stop_id"].to_string();
                            id == stop_query
                                || text(row, "name_en")
                                    .to_lowercase()
                                    .contains(&stop_query.to_lowercase())
                        })
                    else {
                        continue;
                    };
                    let stop_id = row["stop_id"].to_string();
                    let stop_name = text(row, "name_en");
                    let value = self
                        .get(
                            &format!("{GMB}/eta/stop/{stop_id}"),
                            Duration::from_secs(30),
                        )
                        .await?;
                    for route_row in value["data"].as_array().into_iter().flatten() {
                        if route_row["route_id"].as_i64() != Some(route_id)
                            || route_row["route_seq"].as_i64() != Some(sequence)
                        {
                            continue;
                        }
                        for eta in route_row["eta"].as_array().into_iter().flatten() {
                            let timestamp = text(eta, "timestamp");
                            out.push(Eta {
                                operator: "gmb".into(),
                                route: route.into(),
                                direction: sequence.to_string(),
                                destination: label.clone(),
                                stop: stop_name.clone(),
                                minutes: eta["diff"].as_i64(),
                                eta: timestamp,
                                remark: text(eta, "remarks_en"),
                                data_time: text(eta, "timestamp"),
                            });
                        }
                    }
                }
                Ok(out)
            }
            Operator::Nlb => {
                let route_id = self.nlb_route_id(route).await?;
                let value = self
                    .get(
                        &format!("{NLB}/stop.php?action=list&routeId={route_id}"),
                        Duration::from_secs(3600),
                    )
                    .await?;
                let Some(row) = value["stops"].as_array().into_iter().flatten().find(|row| {
                    row["stopId"].as_i64().unwrap_or_default().to_string() == stop_query
                        || text(row, "stopName_e")
                            .to_lowercase()
                            .contains(&stop_query.to_lowercase())
                }) else {
                    bail!("NLB route {route} has no stop matching {stop_query:?}");
                };
                let stop_id = row["stopId"].to_string();
                let stop_name = text(row, "stopName_e");
                let value = self
                    .get(
                        &format!(
                            "{NLB}/stop.php?action=estimatedArrivals&routeId={route_id}&stopId={stop_id}&language=en"
                        ),
                        Duration::from_secs(30),
                    )
                    .await?;
                Ok(value["estimatedArrivals"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|row| {
                        let eta = text(row, "estimatedArrivalTime");
                        Eta {
                            operator: "nlb".into(),
                            route: route.into(),
                            direction: String::new(),
                            destination: String::new(),
                            stop: stop_name.clone(),
                            minutes: minutes_until(&eta),
                            eta,
                            remark: String::new(),
                            data_time: text(row, "generateTime"),
                        }
                    })
                    .collect())
            }
            Operator::MtrBus => {
                let value = self.mtr_schedule(route).await?;
                let mut out = Vec::new();
                for stop in value["busStop"].as_array().into_iter().flatten() {
                    let stop_id = text(stop, "busStopId");
                    if stop_id != stop_query
                        && !stop_id.to_lowercase().contains(&stop_query.to_lowercase())
                    {
                        continue;
                    }
                    for bus in stop["bus"].as_array().into_iter().flatten() {
                        out.push(Eta {
                            operator: "mtrbus".into(),
                            route: route.into(),
                            direction: String::new(),
                            destination: String::new(),
                            stop: stop_id.clone(),
                            eta: text(bus, "arrivalTimeText"),
                            minutes: bus["arrivalTimeInSecond"]
                                .as_str()
                                .and_then(|value| value.parse::<i64>().ok())
                                .map(|seconds| seconds / 60),
                            remark: text(bus, "busRemark"),
                            data_time: String::new(),
                        });
                    }
                }
                Ok(out)
            }
        }
    }

    async fn kmb_variants(
        &self,
        route: &str,
        direction: Option<&str>,
    ) -> Result<Vec<(String, String)>> {
        let value = self
            .get(&format!("{KMB}/route/"), Duration::from_secs(3600))
            .await?;
        let mut variants = Vec::new();
        for row in value["data"].as_array().into_iter().flatten() {
            if text(row, "route").eq_ignore_ascii_case(route) {
                let bound = text(row, "bound");
                if let Some(wanted) = direction
                    && !bound.eq_ignore_ascii_case(wanted)
                {
                    continue;
                }
                variants.push((bound, text(row, "service_type")));
            }
        }
        variants.sort();
        variants.dedup();
        if variants.is_empty() {
            bail!("KMB/LWB route {route} not found");
        }
        Ok(variants)
    }

    async fn kmb_stop(&self, stop_id: &str) -> Result<(String, Option<f64>, Option<f64>)> {
        let value = self
            .get(
                &format!("{KMB}/stop/{stop_id}"),
                Duration::from_secs(86_400),
            )
            .await?;
        let data = &value["data"];
        let name = text(data, "name_en");
        if name.is_empty() {
            bail!("KMB stop {stop_id} not found");
        }
        Ok((name, number(data, "lat"), number(data, "long")))
    }

    async fn kmb_find_stop(
        &self,
        route: &str,
        bound: &str,
        service_type: &str,
        query: &str,
    ) -> Result<Option<(String, String)>> {
        let value = self
            .get(
                &format!(
                    "{KMB}/route-stop/{route}/{}/{service_type}",
                    kmb_word(bound)
                ),
                Duration::from_secs(3600),
            )
            .await?;
        for row in value["data"].as_array().into_iter().flatten() {
            let id = text(row, "stop");
            let (name, _, _) = self.kmb_stop(&id).await?;
            if id == query || name.to_lowercase().contains(&query.to_lowercase()) {
                return Ok(Some((id, name)));
            }
        }
        Ok(None)
    }

    async fn ctb_stop(&self, stop_id: &str) -> Result<(String, Option<f64>, Option<f64>)> {
        let value = self
            .get(
                &format!("{CTB}/stop/{stop_id}"),
                Duration::from_secs(86_400),
            )
            .await?;
        let data = &value["data"];
        Ok((
            text(data, "name_en"),
            number(data, "lat"),
            number(data, "long"),
        ))
    }

    async fn ctb_find_stop(
        &self,
        route: &str,
        direction: &str,
        query: &str,
    ) -> Result<Option<(String, String)>> {
        let value = self
            .get(
                &format!("{CTB}/route-stop/CTB/{route}/{direction}"),
                Duration::from_secs(3600),
            )
            .await?;
        for row in value["data"].as_array().into_iter().flatten() {
            let id = text(row, "stop");
            let (name, _, _) = self.ctb_stop(&id).await?;
            if id == query || name.to_lowercase().contains(&query.to_lowercase()) {
                return Ok(Some((id, name)));
            }
        }
        Ok(None)
    }

    async fn gmb_variants(&self, route: &str) -> Result<(String, i64, Vec<(i64, String)>)> {
        for region in ["HKI", "KLN", "NT"] {
            let Ok(value) = self
                .get(
                    &format!("{GMB}/route/{region}/{route}"),
                    Duration::from_secs(3600),
                )
                .await
            else {
                continue;
            };
            for route_row in value["data"].as_array().into_iter().flatten() {
                let route_id = route_row["route_id"].as_i64().unwrap_or_default();
                let mut variants = Vec::new();
                for row in route_row["directions"].as_array().into_iter().flatten() {
                    let sequence = row["route_seq"].as_i64().unwrap_or_default();
                    let label = format!("{} > {}", text(row, "orig_en"), text(row, "dest_en"));
                    variants.push((sequence, label));
                }
                if route_id != 0 && !variants.is_empty() {
                    return Ok((region.to_string(), route_id, variants));
                }
            }
        }
        bail!("green minibus route {route} not found in HKI, KLN or NT")
    }

    async fn nlb_route_id(&self, route: &str) -> Result<String> {
        let value = self
            .get(
                &format!("{NLB}/route.php?action=list"),
                Duration::from_secs(3600),
            )
            .await?;
        for row in value["routes"].as_array().into_iter().flatten() {
            if text(row, "routeNo").eq_ignore_ascii_case(route) {
                return Ok(row["routeId"].to_string());
            }
        }
        bail!("NLB route {route} not found")
    }

    async fn mtr_schedule(&self, route: &str) -> Result<Value> {
        self.post(
            MTR_BUS,
            serde_json::json!({"language": "en", "routeName": route}),
        )
        .await
    }
}

fn text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn number(value: &Value, key: &str) -> Option<f64> {
    value.get(key).and_then(|value| {
        value
            .as_f64()
            .or_else(|| value.as_str().and_then(|text| text.trim().parse().ok()))
    })
}

struct KmbIndex {
    stops: HashMap<String, (String, f64, f64)>,
    routes: HashMap<(String, String, String), Vec<(i64, String)>>,
    by_stop: HashMap<String, Vec<(String, String, String, i64)>>,
}

fn stop_matches(name: &str, query: &str) -> bool {
    let query = query.trim().to_lowercase();
    !query.is_empty() && name.to_lowercase().contains(&query)
}

fn matching_stops(index: &KmbIndex, query: &str) -> Vec<(String, String, f64, f64)> {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return Vec::new();
    }
    let mut out: Vec<(String, String, f64, f64)> = Vec::new();
    if let Some((name, lat, lng)) = index
        .stops
        .get(query)
        .or_else(|| index.stops.get(&query.to_uppercase()))
    {
        out.push((query.to_uppercase(), name.clone(), *lat, *lng));
    }
    for (id, (name, lat, lng)) in &index.stops {
        if out.iter().any(|(existing, _, _, _)| existing == id) {
            continue;
        }
        if name.to_lowercase().contains(&needle) {
            out.push((id.clone(), name.clone(), *lat, *lng));
        }
    }
    out.sort_by_key(|(_, name, _, _)| {
        let lower = name.to_lowercase();
        (lower != needle, !lower.starts_with(&needle), lower.len())
    });
    // A broad place name such as "Central" otherwise matches buildings like
    // "PARK CENTRAL"; keep the strongest matches only.
    out.truncate(12);
    out
}

fn direct_journeys(
    index: &KmbIndex,
    from_stops: &[(String, String, f64, f64)],
    to_stops: &[(String, String, f64, f64)],
    from: &str,
    to: &str,
) -> Vec<Journey> {
    let to_names: HashMap<&str, &str> = to_stops
        .iter()
        .map(|(id, name, _, _)| (id.as_str(), name.as_str()))
        .collect();
    let mut best: HashMap<(String, String, String, String, String), Journey> = HashMap::new();
    for (from_id, from_name, _, _) in from_stops {
        let Some(entries) = index.by_stop.get(from_id) else {
            continue;
        };
        for (route, bound, service_type, from_sequence) in entries {
            let key = (route.clone(), bound.clone(), service_type.clone());
            let Some(stops) = index.routes.get(&key) else {
                continue;
            };
            let Some((to_sequence, to_id)) = stops
                .iter()
                .filter(|(sequence, id)| {
                    sequence > from_sequence && to_names.contains_key(id.as_str())
                })
                .min_by_key(|(sequence, _)| *sequence)
            else {
                continue;
            };
            let Some(to_name) = to_names.get(to_id.as_str()) else {
                continue;
            };
            let total = to_sequence - from_sequence;
            let journey = Journey {
                from: from.to_string(),
                to: to.to_string(),
                kind: "direct".into(),
                transfers: 0,
                total_stops: total,
                legs: vec![JourneyLeg {
                    operator: "kmb".into(),
                    route: route.clone(),
                    direction: direction_label(bound),
                    service_type: service_type.clone(),
                    board: from_name.clone(),
                    board_id: from_id.clone(),
                    alight: (*to_name).to_string(),
                    alight_id: to_id.clone(),
                    stops: total,
                }],
            };
            best.entry((
                route.clone(),
                bound.clone(),
                service_type.clone(),
                from_id.clone(),
                to_id.clone(),
            ))
            .or_insert(journey);
        }
    }
    best.into_values().collect()
}

fn transfer_journeys(
    index: &KmbIndex,
    from_stops: &[(String, String, f64, f64)],
    to_stops: &[(String, String, f64, f64)],
    from: &str,
    to: &str,
    limit: usize,
) -> Vec<Journey> {
    let to_ids: HashSet<&str> = to_stops.iter().map(|(id, _, _, _)| id.as_str()).collect();
    let mut seen: HashSet<(String, String, String, String, String, String)> = HashSet::new();
    let mut out = Vec::new();
    let mut examined = 0usize;

    'outer: for (from_id, from_name, _, _) in from_stops.iter().take(10) {
        let Some(entries) = index.by_stop.get(from_id) else {
            continue;
        };
        for (route_a, bound_a, service_type_a, from_sequence) in entries {
            let Some(stops_a) =
                index
                    .routes
                    .get(&(route_a.clone(), bound_a.clone(), service_type_a.clone()))
            else {
                continue;
            };
            for (interchange_sequence, interchange_id) in stops_a
                .iter()
                .filter(|(sequence, _)| sequence > from_sequence)
            {
                if to_ids.contains(interchange_id.as_str()) {
                    continue;
                }
                let Some(interchange_entries) = index.by_stop.get(interchange_id) else {
                    continue;
                };
                let Some((interchange_name, _, _)) = index.stops.get(interchange_id) else {
                    continue;
                };
                for (route_b, bound_b, service_type_b, interchange_sequence_b) in
                    interchange_entries
                {
                    if route_b == route_a {
                        continue;
                    }
                    examined += 1;
                    if examined > 100_000 {
                        break 'outer;
                    }
                    let Some(stops_b) = index.routes.get(&(
                        route_b.clone(),
                        bound_b.clone(),
                        service_type_b.clone(),
                    )) else {
                        continue;
                    };
                    let Some((to_sequence, to_id)) = stops_b
                        .iter()
                        .filter(|(sequence, id)| {
                            sequence > interchange_sequence_b && to_ids.contains(id.as_str())
                        })
                        .min_by_key(|(sequence, _)| *sequence)
                    else {
                        continue;
                    };
                    let key = (
                        route_a.clone(),
                        bound_a.clone(),
                        from_id.clone(),
                        route_b.clone(),
                        bound_b.clone(),
                        to_id.clone(),
                    );
                    if !seen.insert(key) {
                        continue;
                    }
                    let Some((to_name, _, _)) = index.stops.get(to_id) else {
                        continue;
                    };
                    let first_leg = interchange_sequence - from_sequence;
                    let second_leg = to_sequence - interchange_sequence_b;
                    out.push(Journey {
                        from: from.to_string(),
                        to: to.to_string(),
                        kind: "one-transfer".into(),
                        transfers: 1,
                        total_stops: first_leg + second_leg,
                        legs: vec![
                            JourneyLeg {
                                operator: "kmb".into(),
                                route: route_a.clone(),
                                direction: direction_label(bound_a),
                                service_type: service_type_a.clone(),
                                board: from_name.clone(),
                                board_id: from_id.clone(),
                                alight: interchange_name.clone(),
                                alight_id: interchange_id.clone(),
                                stops: first_leg,
                            },
                            JourneyLeg {
                                operator: "kmb".into(),
                                route: route_b.clone(),
                                direction: direction_label(bound_b),
                                service_type: service_type_b.clone(),
                                board: interchange_name.clone(),
                                board_id: interchange_id.clone(),
                                alight: to_name.clone(),
                                alight_id: to_id.clone(),
                                stops: second_leg,
                            },
                        ],
                    });
                }
            }
        }
    }
    out.sort_by_key(|journey| journey.total_stops);
    out.truncate(limit);
    out
}

fn direction_label(value: &str) -> String {
    match value.to_lowercase().as_str() {
        "o" | "outbound" | "1" => "outbound".into(),
        "i" | "inbound" | "2" => "inbound".into(),
        other => other.to_string(),
    }
}

fn haversine_km(lat1: f64, lng1: f64, lat2: f64, lng2: f64) -> f64 {
    const EARTH_RADIUS_KM: f64 = 6371.0;
    let delta_lat = (lat2 - lat1).to_radians();
    let delta_lng = (lng2 - lng1).to_radians();
    let a = (delta_lat / 2.0).sin().powi(2)
        + lat1.to_radians().cos() * lat2.to_radians().cos() * (delta_lng / 2.0).sin().powi(2);
    2.0 * EARTH_RADIUS_KM * a.sqrt().asin()
}

fn kmb_word(bound: impl AsRef<str>) -> &'static str {
    if bound.as_ref().eq_ignore_ascii_case("I") {
        "inbound"
    } else {
        "outbound"
    }
}

fn kmb_bound(value: &str) -> String {
    match value.to_lowercase().as_str() {
        "o" | "outbound" | "1" => "O".to_string(),
        "i" | "inbound" | "2" => "I".to_string(),
        other => other.to_uppercase(),
    }
}

fn ctb_direction(value: &str) -> String {
    match value.to_lowercase().as_str() {
        "o" | "outbound" | "1" => "outbound".to_string(),
        "i" | "inbound" | "2" => "inbound".to_string(),
        other => other.to_string(),
    }
}

fn gmb_direction(sequence: i64, value: &str) -> bool {
    match value.to_lowercase().as_str() {
        "1" | "o" | "outbound" => sequence == 1,
        "2" | "i" | "inbound" => sequence == 2,
        other => sequence.to_string() == other,
    }
}

/// Minutes until an ISO timestamp, based on the wall clock.
pub fn minutes_until(timestamp: &str) -> Option<i64> {
    if timestamp.is_empty() {
        return None;
    }
    let eta = chrono::DateTime::parse_from_rfc3339(timestamp)
        .ok()
        .or_else(|| {
            chrono::NaiveDateTime::parse_from_str(timestamp, "%Y-%m-%d %H:%M:%S")
                .ok()
                .and_then(|value| {
                    value
                        .and_local_timezone(chrono::FixedOffset::east_opt(8 * 3600)?)
                        .single()
                })
        })?;
    let now = chrono::Utc::now();
    Some((eta.with_timezone(&chrono::Utc) - now).num_minutes().max(0))
}

/// Reference data for services with no live route or ETA feed.
pub mod private {
    /// MTR Bus routes served by the MTR Bus schedule API.
    pub const MTR_BUS_ROUTES: [(&str, &str, &str); 12] = [
        ("K51", "Tuen Mun Station", "Tai Lam"),
        ("K52", "Tuen Mun Station", "Lung Kwu Tan"),
        ("K53", "Tuen Mun Station", "So Kwun Wat"),
        ("K58", "Tuen Mun Station", "Fu Tai"),
        ("K65", "Tuen Mun Station", "Hung Shui Kiu"),
        ("K66", "Long Ping Station", "Tai Tong"),
        ("K68", "Long Ping Station", "Yuen Long Industrial Estate"),
        ("K73", "Tin Shui Wai Station", "Yuen Long West"),
        ("K74", "Tin Shui Wai Station", "Lau Fau Shan"),
        ("K75A", "Tin Shui Wai Station", "Hung Shui Kiu"),
        ("K75P", "Tin Shui Wai Station", "Hung Shui Kiu"),
        ("K76", "Tin Shui Wai Station", "Tin Heng"),
    ];

    /// Private estate and residents' coach services. There is no
    /// machine-readable ETA feed for these; times are on the operator notice.
    /// The fifth field is the published adult fare, or an empty string when the
    /// operator does not publish it on an accessible page.
    pub const PRIVATE_ROUTES: [(&str, &str, &str, &str, &str); 15] = [
        (
            "Discovery Bay (DBTSL)",
            "DB01A",
            "Tung Chung Station",
            "DB North",
            "HK$17.6 regular / HK$14.2 resident",
        ),
        (
            "Discovery Bay (DBTSL)",
            "DB01R",
            "Tung Chung Station",
            "DB Plaza",
            "HK$17.6 regular / HK$14.2 resident",
        ),
        (
            "Discovery Bay (DBTSL)",
            "DB02A",
            "Hong Kong Airport",
            "DB North",
            "HK$59.1 regular / HK$50.0 resident",
        ),
        (
            "Discovery Bay (DBTSL)",
            "DB02R",
            "Hong Kong Airport",
            "DB Plaza",
            "HK$59.1 regular / HK$50.0 resident",
        ),
        (
            "Discovery Bay (DBTSL)",
            "DB03P",
            "Sunny Bay Station",
            "DB North",
            "HK$17.6 regular / HK$14.2 resident",
        ),
        (
            "Discovery Bay (DBTSL)",
            "DB03R",
            "Sunny Bay Station",
            "DB Plaza",
            "HK$17.6 regular / HK$14.2 resident",
        ),
        (
            "Discovery Bay (DBTSL)",
            "DB08R",
            "Central",
            "Discovery Bay",
            "HK$71.4 regular / HK$51.3 resident (overnight)",
        ),
        (
            "Park Island (PITCL)",
            "NR330",
            "Park Island",
            "Tsing Yi Station",
            "",
        ),
        (
            "Park Island (PITCL)",
            "NR332",
            "Park Island",
            "Kwai Fong",
            "",
        ),
        (
            "Park Island (PITCL)",
            "NR334",
            "Park Island",
            "Airport Terminal 2 / HZMB",
            "",
        ),
        (
            "Park Island (PITCL)",
            "NR338",
            "Park Island",
            "Central Pier 2",
            "Overnight service",
        ),
        (
            "Park Island (PITCL)",
            "NR338S",
            "Ma Wan",
            "Central via Hong Kong Station",
            "",
        ),
        ("Sun Bus", "NR331", "Ma Wan", "Tsuen Wan Station", ""),
        ("Sun Bus", "NR331S", "Ma Wan", "Tsuen Wan West", ""),
        (
            "Sun Bus",
            "NR330A",
            "Park Island",
            "Tsing Yi Station (peak)",
            "",
        ),
    ];

    pub const PRIVATE_NOTE: &str = "Discovery Bay and Park Island publish timetables and notices on their own websites. Routes and fares here are reference data; there is no live ETA feed for these services and this skill does not invent departure times. Check the operator timetable before travel.";
}

/// Red minibus guidance. Routes and fares are published by 16seats.net, but
/// service is not scheduled and there is no live ETA feed.
pub const RED_MINIBUS: &str = "\
Red minibuses have no fixed timetable or live ETA feed. They are 16- or \
19-seat vehicles that pick up and set down along a general corridor, with the \
fare displayed on the windscreen. Route names, published fares and major \
boarding points come from the 16seats.net route directory; exact hours, \
routeing and fares remain at the driver's discretion. Board at a red minibus \
stand or hail one where it is safe and legal, and tell the driver your stop. \
Use `red-route <code or name>` for the published route page.\n";

const RED_DISTRICTS: [(&str, &str); 12] = [
    ("eas", "Eastern District"),
    ("wcc", "Wan Chai, Central & Western"),
    ("sou", "Southern District"),
    ("kwt", "Kwun Tong District"),
    ("kcw", "Kowloon City, Wong Tai Sin"),
    ("ytm", "Yau Tsim Mong District"),
    ("ssp", "Sham Shui Po District"),
    ("tkt", "Tsuen Wan, Kwai Tsing"),
    ("tmy", "Tuen Mun, Yuen Long"),
    ("sha", "Sha Tin District"),
    ("tpn", "Tai Po, North District"),
    ("sak", "Sai Kung District"),
];

fn parse_red_list(
    html: &str,
    district: &str,
    page_url: &str,
    out: &mut Vec<RedRoute>,
) -> Result<()> {
    let base = page_url
        .rsplit_once('/')
        .map(|(base, _)| base)
        .unwrap_or(page_url);
    let mut rest = html;
    while let Some(span) = rest.find("svcname-rmb-list\">") {
        let name_start = span + "svcname-rmb-list\">".len();
        let Some(name_end) = rest[name_start..]
            .find("</span>")
            .map(|end| name_start + end)
        else {
            break;
        };
        let name = clean_text(&rest[name_start..name_end]);
        let Some(anchor) = rest[..span].rfind("<a href=\"r_") else {
            break;
        };
        let href_start = anchor + "<a href=\"".len();
        let Some(href_end) = rest[href_start..].find('"').map(|end| href_start + end) else {
            break;
        };
        let code = rest[href_start..href_end]
            .trim_end_matches(".html")
            .to_string();
        let after = &rest[name_end..];
        let description = after
            .find("<p>")
            .and_then(|start| {
                let start = start + 3;
                after[start..]
                    .find("</p>")
                    .map(|end| clean_text(&after[start..start + end]))
            })
            .unwrap_or_default();
        if !name.is_empty() {
            out.push(RedRoute {
                code: code.clone(),
                district: district.to_string(),
                districts: vec![district.to_string()],
                name,
                description,
                url: format!("{base}/{code}.html"),
            });
        }
        rest = &rest[name_end..];
    }
    Ok(())
}

/// Merge duplicate route codes so a route keeps every district page it appears on.
fn dedupe_red_routes(routes: Vec<RedRoute>) -> Vec<RedRoute> {
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut unique: Vec<RedRoute> = Vec::new();
    for route in routes {
        if let Some(&position) = index.get(&route.code) {
            let existing = &mut unique[position];
            for district in route.districts {
                if !existing.districts.contains(&district) {
                    existing.districts.push(district);
                }
            }
        } else {
            index.insert(route.code.clone(), unique.len());
            unique.push(route);
        }
    }
    unique
}

fn clean_route_page(html: &str) -> String {
    let mut body = html.to_string();
    for tag in ["script", "style", "noscript"] {
        loop {
            let open = format!("<{tag}");
            let Some(start) = body.find(&open) else { break };
            let Some(close) = body[start..]
                .find(&format!("</{tag}>"))
                .map(|end| start + end + tag.len() + 3)
            else {
                break;
            };
            body.replace_range(start..close, " ");
        }
    }
    // Keep paragraph boundaries before stripping the remaining tags.
    for (tag, replacement) in [
        ("<br>", "\n"),
        ("<br/>", "\n"),
        ("<br />", "\n"),
        ("</p>", "\n"),
        ("</div>", "\n"),
        ("</li>", "\n"),
        ("</h1>", "\n"),
        ("</h2>", "\n"),
        ("</h3>", "\n"),
        ("</tr>", "\n"),
        ("</td>", " "),
    ] {
        body = body.replace(tag, replacement);
    }
    let mut text = String::with_capacity(body.len());
    let mut in_tag = false;
    for character in body.chars() {
        match character {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                text.push(' ');
            }
            _ if !in_tag => text.push(character),
            _ => {}
        }
    }
    let text = decode_entities(&text);
    let start = text
        .rfind("Red Minibus Route")
        .map(|index| index + "Red Minibus Route".len())
        .unwrap_or(0);
    let end = text.find("CONTACT US").unwrap_or(text.len());
    let mut lines: Vec<String> = text[start..end]
        .lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|line| {
            !line.is_empty()
                && !line.starts_with("Advertisement // scroll to continue")
                && line != "&nbsp;"
        })
        .collect();
    // Drop the repeated page heading, keeping the route name and substance.
    if lines.first().is_some_and(|line| line.is_empty()) {
        lines.remove(0);
    }
    lines.join("\n")
}

fn clean_text(html: &str) -> String {
    let mut text = String::with_capacity(html.len());
    let mut in_tag = false;
    for character in html.chars() {
        match character {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => text.push(character),
            _ => {}
        }
    }
    decode_entities(&text)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn decode_entities(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(index) = rest.find('&') {
        out.push_str(&rest[..index]);
        let tail = &rest[index..];
        let Some(end) = tail.find(';').filter(|end| *end <= 10) else {
            out.push('&');
            rest = &tail[1..];
            continue;
        };
        let entity = &tail[1..end];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" | "#39" => Some('\''),
            "nbsp" => Some(' '),
            "harr" => Some('\u{2194}'),
            "rarr" => Some('\u{2192}'),
            "larr" => Some('\u{2190}'),
            "#9672" => Some('\u{25c8}'),
            other => other
                .strip_prefix('#')
                .and_then(|value| {
                    if let Some(hex) = value.strip_prefix(['x', 'X']) {
                        u32::from_str_radix(hex, 16).ok()
                    } else {
                        value.parse::<u32>().ok()
                    }
                })
                .and_then(char::from_u32),
        };
        match decoded {
            Some(character) => out.push(character),
            None => out.push_str(&tail[..=end]),
        }
        rest = &tail[end + 1..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_operator_aliases() {
        assert_eq!(Operator::parse("Citybus").unwrap(), Operator::Ctb);
        assert_eq!(Operator::parse("mtr-bus").unwrap(), Operator::MtrBus);
        assert!(Operator::parse("tram").is_err());
    }

    #[test]
    fn normalises_directions() {
        assert_eq!(kmb_bound("outbound"), "O");
        assert_eq!(ctb_direction("I"), "inbound");
        assert!(gmb_direction(1, "outbound"));
        assert!(gmb_direction(2, "2"));
    }

    #[test]
    fn parses_red_minibus_directory_and_entities() {
        let html = r#"<a href="r_h08.html"><div><p><span class="svcname-rmb-list">Kennedy Town - Shau Kei Wan</span></p><p>Shau Kei Wan &harr; Kennedy Town <span>Overnight</span></p></div></a>"#;
        let mut routes = Vec::new();
        parse_red_list(
            html,
            "Eastern District",
            "https://www.16seats.net/eng/rmb/r_eas.html",
            &mut routes,
        )
        .unwrap();
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].code, "r_h08");
        assert_eq!(
            routes[0].description,
            "Shau Kei Wan \u{2194} Kennedy Town Overnight"
        );
        assert_eq!(routes[0].url, "https://www.16seats.net/eng/rmb/r_h08.html");
        assert_eq!(decode_entities("A &amp; B &#65;"), "A & B A");
    }

    #[test]
    fn dedupe_keeps_every_district_for_a_route() {
        let route = |district: &str| RedRoute {
            code: "r_n52".into(),
            district: district.into(),
            districts: vec![district.into()],
            name: "So Kwun Wat - Tai Wai".into(),
            description: String::new(),
            url: "https://www.16seats.net/eng/rmb/r_n52.html".into(),
        };
        let routes = dedupe_red_routes(vec![
            route("Tuen Mun, Yuen Long"),
            route("Sha Tin District"),
        ]);
        assert_eq!(routes.len(), 1);
        assert_eq!(
            routes[0].districts,
            vec!["Tuen Mun, Yuen Long", "Sha Tin District"]
        );
    }

    #[test]
    fn cleans_navigation_from_a_red_route_page() {
        let html = r#"<html><body>Directory Red Minibus Route Directory
            Red Minibus Route Kennedy Town - Shau Kei Wan Regular Timetable Daily
            Regular Fare Table Full fare $8.0 CONTACT US</body></html>"#;
        let text = clean_route_page(html);
        assert!(text.starts_with("Kennedy Town - Shau Kei Wan"));
        assert!(text.contains("Regular Fare Table"));
        assert!(!text.contains("Directory"));
    }

    #[test]
    fn computes_minutes_for_hkt_timestamps() {
        let now = chrono::Utc::now() + chrono::Duration::minutes(5);
        let stamp = now.to_rfc3339();
        let minutes = minutes_until(&stamp).unwrap();
        assert!((4..=5).contains(&minutes));
    }

    #[test]
    fn measures_distance_between_two_hong_kong_points() {
        let km = haversine_km(22.2819, 114.1582, 22.2976, 114.1722);
        assert!((1.5..3.5).contains(&km), "unexpected distance {km}");
    }

    fn synthetic_index() -> KmbIndex {
        let mut stops = HashMap::new();
        stops.insert("A".to_string(), ("Alpha Estate".to_string(), 22.30, 114.16));
        stops.insert("B".to_string(), ("Bravo Road".to_string(), 22.31, 114.17));
        stops.insert("C".to_string(), ("Charlie Bay".to_string(), 22.32, 114.18));

        let mut routes = HashMap::new();
        routes.insert(
            ("1".to_string(), "O".to_string(), "1".to_string()),
            vec![(1, "A".to_string()), (2, "B".to_string())],
        );
        routes.insert(
            ("2".to_string(), "I".to_string(), "1".to_string()),
            vec![(1, "B".to_string()), (2, "C".to_string())],
        );

        let mut by_stop = HashMap::new();
        by_stop.insert(
            "A".to_string(),
            vec![("1".to_string(), "O".to_string(), "1".to_string(), 1)],
        );
        by_stop.insert(
            "B".to_string(),
            vec![
                ("1".to_string(), "O".to_string(), "1".to_string(), 2),
                ("2".to_string(), "I".to_string(), "1".to_string(), 1),
            ],
        );
        by_stop.insert(
            "C".to_string(),
            vec![("2".to_string(), "I".to_string(), "1".to_string(), 2)],
        );

        KmbIndex {
            stops,
            routes,
            by_stop,
        }
    }

    #[test]
    fn finds_a_direct_kmb_journey() {
        let index = synthetic_index();
        let from = matching_stops(&index, "Alpha");
        let to = matching_stops(&index, "Bravo");
        let journeys = direct_journeys(&index, &from, &to, "Alpha", "Bravo");
        assert_eq!(journeys.len(), 1);
        assert_eq!(journeys[0].kind, "direct");
        assert_eq!(journeys[0].total_stops, 1);
        assert_eq!(journeys[0].legs[0].route, "1");
        assert_eq!(journeys[0].legs[0].alight, "Bravo Road");
    }

    #[test]
    fn finds_a_one_transfer_kmb_journey() {
        let index = synthetic_index();
        let from = matching_stops(&index, "Alpha");
        let to = matching_stops(&index, "Charlie");
        assert!(direct_journeys(&index, &from, &to, "Alpha", "Charlie").is_empty());
        let journeys = transfer_journeys(&index, &from, &to, "Alpha", "Charlie", 5);
        assert_eq!(journeys.len(), 1);
        assert_eq!(journeys[0].transfers, 1);
        assert_eq!(journeys[0].total_stops, 2);
        assert_eq!(journeys[0].legs.len(), 2);
        assert_eq!(journeys[0].legs[1].alight, "Charlie Bay");
    }

    #[test]
    fn prefers_stop_names_that_start_with_the_query() {
        let index = synthetic_index();
        let matches = matching_stops(&index, "Charlie");
        assert_eq!(matches[0].1, "Charlie Bay");
    }
}
