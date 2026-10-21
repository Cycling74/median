//! Registration and notification: `object_register`, `object_findregistered`,
//! `object_unregister`, `object_subscribe`, `object_unsubscribe`, `object_attach`,
//! `object_detach`, `object_notify`, plus the obex store (`object_obex_lookup`,
//! `object_obex_storeflags`) and `object_attr_touch`.
//!
//! Clients receive notifications through their `notify` method (`A_CANT`, called as
//! `notify(x, sender_name, message, sender, data)`). A client that subscribed before the object
//! was registered is attached and sent `globalsymbol_binding` (data: the object) when it is; when
//! the object is unregistered or freed, attached clients are sent `globalsymbol_unbinding`.

use crate::class::{Raw, call_raw};
use crate::state::{State, Subscription, state};
use crate::symbols::{intern, name_of, sym_name};
use max_sys::{e_max_errorcodes as err, t_max_err, t_object, t_symbol};
use std::ffi::{c_long, c_void};

fn e(code: err::Type) -> t_max_err {
    code as t_max_err
}

/// What to call, collected under the lock and run after it is released.
struct Notification {
    client: usize,
    fun: usize,
    name: usize,
    msg: usize,
    sender: usize,
    data: usize,
}

fn notification(
    st: &State,
    client: usize,
    name: &str,
    msg: &str,
    sender: usize,
    data: usize,
) -> Option<Notification> {
    let obj = st.objects.get(&client)?;
    let fun = st.classes[&obj.class].methods.get("notify")?.fun;
    Some(Notification {
        client,
        fun,
        name: intern(name.as_bytes()) as usize,
        msg: intern(msg.as_bytes()) as usize,
        sender,
        data,
    })
}

fn deliver(list: Vec<Notification>) {
    for n in list {
        // SAFETY: `notify` methods are registered `A_CANT` with `(x, s, msg, sender, data)`
        unsafe {
            call_raw(
                n.fun,
                &[
                    Raw::I(n.client),
                    Raw::I(n.name),
                    Raw::I(n.msg),
                    Raw::I(n.sender),
                    Raw::I(n.data),
                ],
            );
        }
    }
}

fn attach(st: &mut State, obj: usize, client: usize) {
    let clients = st.attached.entry(obj).or_default();
    if !clients.contains(&client) {
        clients.push(client);
    }
}

fn class_name_of(st: &State, obj: usize) -> Option<String> {
    st.objects.get(&obj).map(|o| st.classes[&o.class].name.clone())
}

/// # Safety
/// Symbols from `gensym`; `x` a live object (anything else is reported and returns null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn object_register(
    name_space: *mut t_symbol,
    s: *mut t_symbol,
    x: *mut c_void,
) -> *mut c_void {
    // SAFETY: the caller's contract
    let (ns, name) = unsafe { (sym_name(name_space), sym_name(s)) };
    let addr = x as usize;
    let class_name = {
        let mut st = state();
        if !st.objects.contains_key(&addr) {
            st.post_error(addr, format!("median-mock: object_register of {addr:#x}, which is not a live object"));
            return std::ptr::null_mut();
        }
        if let Some(o) = st.objects.get_mut(&addr) {
            o.registered = Some((ns.clone(), name.clone()));
        }
        class_name_of(&st, addr)
    };
    register_with_class(&ns, &name, addr, class_name.as_deref());
    x
}

/// Register `addr` under `(ns, name)`, attach the subscribers waiting for it and send them the
/// binding notification. `class_name` is what a subscription's class filter is compared to.
pub(crate) fn register_with_class(ns: &str, name: &str, addr: usize, class_name: Option<&str>) {
    let mut to_notify = Vec::new();
    {
        let mut st = state();
        st.registry.insert((ns.to_string(), name.to_string()), addr);
        let waiting: Vec<usize> = st
            .subscriptions
            .iter()
            .filter(|sub| {
                sub.namespace == ns
                    && sub.name == name
                    && sub.classname.as_deref().is_none_or(|c| Some(c) == class_name)
            })
            .map(|sub| sub.client)
            .collect();
        for client in waiting {
            attach(&mut st, addr, client);
            to_notify.extend(notification(&st, client, name, "globalsymbol_binding", addr, addr));
        }
    }
    deliver(to_notify);
}

/// # Safety
/// Symbols from `gensym`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn object_findregistered(
    name_space: *mut t_symbol,
    s: *mut t_symbol,
) -> *mut c_void {
    // SAFETY: the caller's contract
    let key = unsafe { (sym_name(name_space), sym_name(s)) };
    state()
        .registry
        .get(&key)
        .map_or(std::ptr::null_mut(), |&o| o as *mut c_void)
}

fn unregister(addr: usize) -> t_max_err {
    let mut to_notify = Vec::new();
    {
        let mut st = state();
        let Some(reg) = st
            .registry
            .iter()
            .find_map(|(k, &v)| (v == addr).then(|| k.clone()))
        else {
            return e(err::MAX_ERR_GENERIC);
        };
        st.registry.remove(&reg);
        if let Some(o) = st.objects.get_mut(&addr) {
            o.registered = None;
        }
        let clients = st.attached.remove(&addr).unwrap_or_default();
        for client in clients {
            to_notify.extend(notification(&st, client, &reg.1, "globalsymbol_unbinding", addr, addr));
        }
    }
    deliver(to_notify);
    e(err::MAX_ERR_NONE)
}

/// # Safety
/// `x` must be a registered live object.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn object_unregister(x: *mut c_void) -> t_max_err {
    unregister(x as usize)
}

/// # Safety
/// Symbols from `gensym` (`classname` may be null); `x` a live object.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn object_subscribe(
    name_space: *mut t_symbol,
    s: *mut t_symbol,
    classname: *mut t_symbol,
    x: *mut c_void,
) -> *mut c_void {
    // SAFETY: the caller's contract
    let (ns, name, class) = unsafe { (sym_name(name_space), sym_name(s), sym_name(classname)) };
    let classname = if class.is_empty() { None } else { Some(class) };
    let mut st = state();
    st.subscriptions.push(Subscription {
        namespace: ns.clone(),
        name: name.clone(),
        classname: classname.clone(),
        client: x as usize,
    });
    let Some(&obj) = st.registry.get(&(ns, name)) else {
        return std::ptr::null_mut();
    };
    if classname.is_some() && class_name_of(&st, obj) != classname {
        return std::ptr::null_mut();
    }
    attach(&mut st, obj, x as usize);
    obj as *mut c_void
}

/// # Safety
/// As `object_subscribe`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn object_unsubscribe(
    name_space: *mut t_symbol,
    s: *mut t_symbol,
    _classname: *mut t_symbol,
    x: *mut c_void,
) -> t_max_err {
    // SAFETY: the caller's contract
    let (ns, name) = unsafe { (sym_name(name_space), sym_name(s)) };
    let mut st = state();
    st.subscriptions
        .retain(|sub| !(sub.namespace == ns && sub.name == name && sub.client == x as usize));
    if let Some(&obj) = st.registry.get(&(ns, name)) {
        if let Some(clients) = st.attached.get_mut(&obj) {
            clients.retain(|&c| c != x as usize);
        }
    }
    e(err::MAX_ERR_NONE)
}

/// # Safety
/// Symbols from `gensym`; `x` a live object.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn object_attach(
    name_space: *mut t_symbol,
    s: *mut t_symbol,
    x: *mut c_void,
) -> *mut c_void {
    // SAFETY: the caller's contract
    let key = unsafe { (sym_name(name_space), sym_name(s)) };
    let mut st = state();
    let Some(&obj) = st.registry.get(&key) else {
        return std::ptr::null_mut();
    };
    attach(&mut st, obj, x as usize);
    obj as *mut c_void
}

/// # Safety
/// As `object_attach`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn object_detach(
    name_space: *mut t_symbol,
    s: *mut t_symbol,
    x: *mut c_void,
) -> t_max_err {
    // SAFETY: the caller's contract
    let key = unsafe { (sym_name(name_space), sym_name(s)) };
    let mut st = state();
    let Some(&obj) = st.registry.get(&key) else {
        return e(err::MAX_ERR_GENERIC);
    };
    if let Some(clients) = st.attached.get_mut(&obj) {
        clients.retain(|&c| c != x as usize);
    }
    e(err::MAX_ERR_NONE)
}

/// Sends `s` to the `notify` method of every client attached to `x`.
///
/// # Safety
/// `x` must be a registered live object, `s` a symbol.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn object_notify(
    x: *mut c_void,
    s: *mut t_symbol,
    data: *mut c_void,
) -> t_max_err {
    let msg = name_of(s as usize);
    let list = {
        let st = state();
        let Some(reg) = st
            .registry
            .iter()
            .find_map(|(k, &v)| (v == x as usize).then(|| k.1.clone()))
        else {
            return e(err::MAX_ERR_GENERIC);
        };
        st.attached
            .get(&(x as usize))
            .into_iter()
            .flatten()
            .filter_map(|&c| notification(&st, c, &reg, &msg, x as usize, data as usize))
            .collect::<Vec<_>>()
    };
    deliver(list);
    e(err::MAX_ERR_NONE)
}

/// # Safety
/// `x` a live object, `key` a symbol, `val` valid for writing a pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn object_obex_lookup(
    x: *mut c_void,
    key: *mut t_symbol,
    val: *mut *mut t_object,
) -> t_max_err {
    let key = name_of(key as usize);
    match state()
        .objects
        .get(&(x as usize))
        .and_then(|o| o.obex.get(&key))
    {
        Some(&(v, _)) if !val.is_null() => {
            // SAFETY: the caller's contract
            unsafe { val.write(v as *mut t_object) };
            e(err::MAX_ERR_NONE)
        }
        _ => e(err::MAX_ERR_GENERIC),
    }
}

/// # Safety
/// `x` a live object, `key` a symbol.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn object_obex_storeflags(
    x: *mut c_void,
    key: *mut t_symbol,
    val: *mut t_object,
    flags: c_long,
) -> t_max_err {
    let key = name_of(key as usize);
    match state().objects.get_mut(&(x as usize)) {
        Some(o) => {
            o.obex.insert(key, (val as usize, flags as i64));
            e(err::MAX_ERR_NONE)
        }
        None => e(err::MAX_ERR_INVALID_PTR),
    }
}

/// Records the touch, see [`crate::Kernel::attr_touches`].
///
/// # Safety
/// `x` a live object, `attrname` a symbol.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn object_attr_touch(x: *mut t_object, attrname: *mut t_symbol) -> t_max_err {
    let name = name_of(attrname as usize);
    state().touches.push((x as usize, name));
    e(err::MAX_ERR_NONE)
}

/// Unregister whatever is registered at `addr` (notifying its clients); an error code if nothing is.
pub(crate) fn unregister_addr(addr: usize) -> t_max_err {
    unregister(addr)
}
