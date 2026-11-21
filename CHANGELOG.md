# Changelog

All notable changes are documented here. The format follows Keep a Changelog
and the project uses semantic versioning.

## [Unreleased]

### Added

- `near`: stops within a radius of a WGS84 coordinate, with distances. KMB
  works from its bulk stop list; Citybus and NLB need `--route`; GMB and MTR
  Bus publish no stop coordinates.
- `journey`: direct and one-transfer planning across the published KMB
  route-stop graph, with a Citybus/NLB fallback when KMB has no route. GMB
  route discovery is not covered because the feed has no bulk route-stop
  endpoint.
- MCP tools `bus_stops_near` and `bus_journey`, and the missing
  `red_minibus_info` declaration.

### Fixed

- The MCP server now returns a `-32700` parse error for malformed JSON instead
  of silently dropping the line.

## [0.1.0] - 2026-11-22

- Initial release of the `hkbus` CLI and MCP server.
- Standardized installers, Docker/Apple container instructions and CI.
