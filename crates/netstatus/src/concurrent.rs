/// Runs `f` over `items` concurrently, one thread per item, returning
/// results in the same order.
pub(crate) fn map_all<'a, R: Send>(items: &[&'a str], f: impl Fn(&'a str) -> R + Sync) -> Vec<R> {
    std::thread::scope(|scope| {
        let handles: Vec<_> = items.iter().map(|&item| scope.spawn(|| f(item))).collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    })
}
