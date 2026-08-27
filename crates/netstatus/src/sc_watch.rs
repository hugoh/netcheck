//! Event-driven notification when the System Configuration dynamic store
//! changes — interface up/down, IP/DNS reconfiguration, VPN connect. Unlike
//! `sc_store.rs`'s one-shot reads, this keeps a dedicated thread blocked in
//! a `CFRunLoop`, so callers pay nothing until an actual change occurs (no
//! polling), for cases where a subscription is cheaper than the poll it
//! would otherwise replace.

use core_foundation::array::CFArray;
use core_foundation::runloop::{CFRunLoop, kCFRunLoopDefaultMode};
use core_foundation::string::CFString;
use std::io;
use std::sync::mpsc;
use std::thread::JoinHandle;
use system_configuration::dynamic_store::{SCDynamicStoreBuilder, SCDynamicStoreCallBackContext};

type OnChange = Box<dyn Fn() + Send>;

/// A running watcher. Dropping this does not stop the watcher — call
/// [`WatchHandle::stop`] explicitly, or let the watcher run for the process
/// lifetime (the common case: it's cheap to leave running).
pub struct WatchHandle {
    run_loop: CFRunLoop,
    thread: Option<JoinHandle<()>>,
}

impl WatchHandle {
    /// Stops the watcher's run loop and waits for its thread to exit.
    pub fn stop(mut self) {
        self.run_loop.stop();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn callback(
    _store: system_configuration::dynamic_store::SCDynamicStore,
    _changed_keys: CFArray<CFString>,
    on_change: &mut OnChange,
) {
    on_change();
}

/// Spawns a thread that watches for global IPv4/DNS changes and per-interface
/// link/IPv4 changes, calling `on_change` (from that thread) on every fire.
/// `on_change` isn't told which key changed — a config change is a config
/// change, and the caller already knows how to run a full recheck.
pub fn watch_config_changes(on_change: impl Fn() + Send + 'static) -> io::Result<WatchHandle> {
    let (loop_tx, loop_rx) = mpsc::channel::<CFRunLoop>();

    let thread = std::thread::spawn(move || {
        let context = SCDynamicStoreCallBackContext {
            callout: callback,
            info: Box::new(on_change) as OnChange,
        };
        let Some(store) = SCDynamicStoreBuilder::new("netstatus-watch")
            .callback_context(context)
            .build()
        else {
            return;
        };

        let keys = CFArray::from_CFTypes(&[
            CFString::from("State:/Network/Global/IPv4"),
            CFString::from("State:/Network/Global/DNS"),
        ]);
        let patterns = CFArray::from_CFTypes(&[
            CFString::from("State:/Network/Interface/[^/]+/Link"),
            CFString::from("State:/Network/Interface/[^/]+/IPv4"),
        ]);
        store.set_notification_keys(&keys, &patterns);

        let Some(run_loop_source) = store.create_run_loop_source() else {
            return;
        };
        let run_loop = CFRunLoop::get_current();
        run_loop.add_source(&run_loop_source, unsafe { kCFRunLoopDefaultMode });

        if loop_tx.send(run_loop).is_err() {
            return;
        }
        CFRunLoop::run_current();
    });

    let run_loop = loop_rx
        .recv()
        .map_err(|_| io::Error::other("SCDynamicStore watcher thread failed to start"))?;

    Ok(WatchHandle {
        run_loop,
        thread: Some(thread),
    })
}
