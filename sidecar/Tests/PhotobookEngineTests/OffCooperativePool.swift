import Foundation

/// Runs `body` on its own kernel thread and suspends the calling task until
/// it returns. Every test that reaches a real `VNImageRequestHandler.perform()`
/// must go through this.
///
/// `perform()` blocks its caller until work Vision schedules internally
/// completes, and that work needs a Swift-concurrency cooperative-pool
/// thread. swift-testing runs test bodies on that same pool, which is only
/// as wide as the core count, so a synchronous test blocked in Vision holds
/// one pool thread hostage. On a 3-core GitHub macos-15 runner three such
/// tests occupied the whole pool and hung the suite until the job was
/// cancelled: `sample` showed all three pool threads in
/// `VNControlledCapacityTasksQueue dispatchGroupWait`, and no thread running
/// the Vision work they waited on. Suspending here frees the pool thread.
///
/// Reproduce on any Mac by narrowing the pool to one thread:
/// `LIBDISPATCH_COOPERATIVE_POOL_STRICT=1 swift test --package-path sidecar`.
///
/// The sidecar itself never calls Vision from the cooperative pool
/// (`main.swift` is synchronous), so this is a test-harness concern only.
func offCooperativePool<T>(_ body: @escaping @Sendable () -> T) async -> T {
    let box = await withCheckedContinuation { (done: CheckedContinuation<UncheckedBox<T>, Never>) in
        let thread = Thread { done.resume(returning: UncheckedBox(value: body())) }
        thread.stackSize = 1 << 20
        thread.start()
    }
    return box.value
}

/// The result crosses from the worker thread to the awaiting task exactly
/// once, after the worker has finished with it, so no state is shared.
struct UncheckedBox<T>: @unchecked Sendable {
    let value: T
}
