// Ticket dispatch with sealed results.
package dispatch

import kotlin.math.max

sealed class Outcome {
    data class Accepted(val id: Int) : Outcome()
    data class Rejected(val reason: String) : Outcome()
}

fun dispatch(load: Int, capacity: Int = 64): Outcome {
    val slack = max(0, capacity - load)
    return when {
        slack > 0 -> Outcome.Accepted(id = load + 1)
        else -> Outcome.Rejected("over capacity by ${load - capacity}")
    }
}

fun main() {
    val results = listOf(12, 80, 63).map { dispatch(it) }
    results.forEach(::println)
}
