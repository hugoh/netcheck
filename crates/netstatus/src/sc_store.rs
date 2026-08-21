//! Small shared helpers for reading typed dictionaries out of the System
//! Configuration dynamic store (`SCDynamicStore`), used by `vpn.rs`,
//! `dns.rs`, and (indirectly, via `SCDynamicStore::get_proxies`) `proxy.rs`.

use core_foundation::array::CFArray;
use core_foundation::base::{CFType, FromVoid, TCFType};
use core_foundation::dictionary::CFDictionary;
use core_foundation::string::CFString;
use std::ffi::c_void;
use system_configuration::dynamic_store::SCDynamicStore;

pub(crate) type Dict = CFDictionary<CFString, CFType>;

/// Reads `key` from the dynamic store as a typed dictionary.
///
/// `SCDynamicStore::get` only exposes an opaque `CFPropertyList`, and
/// `CFPropertyList::downcast_into` only supports the type-erased
/// `CFDictionary<*const c_void, *const c_void>` — not the `CFString`/`CFType`
/// pairing this crate wants to `.find()` keys on. Reinterpreting the phantom
/// type parameters via `wrap_under_get_rule` on the same underlying
/// `CFDictionaryRef` is safe: the ref's runtime representation doesn't
/// depend on `K`/`V`, only Rust's view of it does.
pub(crate) fn get_dict(store: &SCDynamicStore, key: &str) -> Option<Dict> {
    let opaque: CFDictionary = store.get(key)?.downcast_into()?;
    Some(unsafe { CFDictionary::wrap_under_get_rule(opaque.as_concrete_TypeRef()) })
}

pub(crate) fn cf_string(dict: &Dict, key: &str) -> Option<String> {
    dict.find(CFString::from(key))
        .and_then(|v| v.downcast::<CFString>())
        .map(|s| s.to_string())
}

pub(crate) fn cf_string_array(dict: &Dict, key: &str) -> Vec<String> {
    dict.find(CFString::from(key))
        .and_then(|v| v.downcast::<CFArray<*const c_void>>())
        .map(|arr| {
            arr.iter()
                .filter_map(|ptr| {
                    let item = unsafe { CFType::from_void(*ptr) };
                    item.downcast::<CFString>()
                })
                .map(|s| s.to_string())
                .collect()
        })
        .unwrap_or_default()
}

/// Reads an array-of-dictionaries value (e.g. `AdditionalRoutes`, each
/// element `{DestinationAddress, SubnetMask}`), reinterpreting each opaque
/// element the same way `get_dict` reinterprets the top-level value.
pub(crate) fn cf_dict_array(dict: &Dict, key: &str) -> Vec<Dict> {
    dict.find(CFString::from(key))
        .and_then(|v| v.downcast::<CFArray<*const c_void>>())
        .map(|arr| {
            arr.iter()
                .filter_map(|ptr| {
                    let item = unsafe { CFType::from_void(*ptr) };
                    let opaque: CFDictionary = item.downcast::<CFDictionary>()?;
                    Some(unsafe { CFDictionary::wrap_under_get_rule(opaque.as_concrete_TypeRef()) })
                })
                .collect()
        })
        .unwrap_or_default()
}
