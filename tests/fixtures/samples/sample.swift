// Weather summary over a rolling window.
import Foundation

struct Reading {
    let celsius: Double
    let at: Date
}

enum Trend: String {
    case rising, falling, steady
}

func trend(of readings: [Reading]) -> Trend {
    guard let first = readings.first, let last = readings.last else {
        return .steady
    }
    let delta = last.celsius - first.celsius
    if abs(delta) < 0.5 { return .steady }
    return delta > 0 ? .rising : .falling
}

let window = [Reading(celsius: 18.2, at: .now), Reading(celsius: 21.7, at: .now)]
print("trend: \(trend(of: window).rawValue)")
