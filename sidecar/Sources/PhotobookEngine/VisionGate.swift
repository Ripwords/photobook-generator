import Foundation

/// Throttles concurrent `VNImageRequestHandler.perform()` calls across the
/// whole process. Vision **deadlocks** on `VNControlledCapacityTasksQueue`
/// when too many handlers are in flight at once -- a real, reproduced
/// deadlock (zero CPU progress, confirmed via `sample`), not a hypothetical
/// one. See `docs/PROJECT-STATUS.md`'s "Apple Vision" trap and
/// `AnalyzerTests.swift`.
///
/// Originally this guard lived inline in `Analyzer.analyzeOne` as a private
/// semaphore. It was extracted here after `Benchmarker.benchmarkOne` called
/// `VisionAnalyzer.analyze` directly, unguarded -- and reproduced the exact
/// same deadlock the moment `BenchmarkerTests` ran alongside the rest of the
/// suite under swift-testing's default parallel test execution. Closing that
/// gap alone wasn't enough: `VisionAnalyzerTests.swift` had three more call
/// sites that were ALSO ungated, and the deadlock still reproduced with only
/// those three plus the (now-gated) production call sites in play. All of
/// them now go through this gate. Any call site that invokes
/// `VisionAnalyzer.analyze` MUST go through `VisionGate.run`, not call it
/// directly -- there is no compiler check for this, only this comment and
/// the call sites (`Analyzer.analyzeOne`, `Benchmarker.benchmarkOne`, and
/// the three in `VisionAnalyzerTests.swift`) that currently obey it.
///
/// Scope this to exactly the Vision call, nothing else -- metrics and
/// decode are CPU-bound and must run unguarded at full width. 4 was chosen
/// after measuring throughput against 2 (see `Analyzer`'s original comment,
/// task-10-report.md); re-measure before changing it.
enum VisionGate {
    private static let concurrencyLimit = 4
    private static let semaphore = DispatchSemaphore(value: concurrencyLimit)

    static func run<T>(_ body: () -> T) -> T {
        semaphore.wait()
        defer { semaphore.signal() }
        return body()
    }
}
