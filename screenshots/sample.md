<p align="center">
<img src="https://img.shields.io/badge/build-passing-brightgreen" height="20">
<img src="https://img.shields.io/badge/version-1.4.2-blue" height="20">
<img src="https://img.shields.io/badge/coverage-96%25-yellowgreen" height="20">
<img src="https://img.shields.io/badge/license-MIT-orange" height="20">
</p>

# Hoopoe

Hoopoe is a small command line planner for hiking trips. It reads a folder
of GPX tracks, estimates walking time with **Naismith's rule**, and prints
a day-by-day plan you can carry offline. It exists because paper maps do
not compute and phone apps do not respect batteries.

> [!TIP]
> Keep one track per day. Hoopoe joins consecutive days automatically when
> a track ends within 500 m of the next one's start.

## Quick start

```rust
// Estimate a day: distance plus one minute per ten meters of climb.
fn walking_minutes(distance_km: f64, ascent_m: f64) -> f64 {
    distance_km * 12.0 + ascent_m / 10.0
}
```

| Day | Track | Distance | Ascent | Estimate |
|-----|-------|----------|--------|----------|
| 1 | Col des Aiguilles | 14.2 km | 980 m | 4 h 30 |
| 2 | Lac Blanc | 11.8 km | 620 m | 3 h 25 |
| 3 | Descent to Argentière | 9.6 km | 120 m | 2 h 10 |

Planned for the season[^1]:

- [x] GPX parsing and joining
- [x] Time estimates with rest stops
- [ ] Weather window suggestions
- [ ] Printable elevation profiles, $y = f(x)$ sampled every 50 m

[^1]: Subject to snow conditions above 2400 m.
