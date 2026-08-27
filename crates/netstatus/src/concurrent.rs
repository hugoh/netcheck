/// Runs `f` over `items` concurrently, one thread per item, returning
/// results in the same order.
pub(crate) fn map_all<'a, R: Send>(items: &[&'a str], f: impl Fn(&'a str) -> R + Sync) -> Vec<R> {
    std::thread::scope(|scope| {
        let handles: Vec<_> = items.iter().map(|&item| scope.spawn(|| f(item))).collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    })
}

/// Runs `f` over `items` concurrently, one thread per item, calling
/// `on_result` with each item's result as soon as it's ready — not
/// necessarily in input order — instead of waiting for every item to finish
/// before any result is visible. Blocks until every item has completed.
pub(crate) fn for_each_concurrent<'a, R: Send>(
    items: &[&'a str],
    f: impl Fn(&'a str) -> R + Sync,
    on_result: impl Fn(R) + Sync,
) {
    std::thread::scope(|scope| {
        for &item in items {
            scope.spawn(|| on_result(f(item)));
        }
    });
}
