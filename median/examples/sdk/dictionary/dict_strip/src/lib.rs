//! A port of the SDK's `dictionary/dict.strip`: removes the keys its `keys` attribute names from a
//! dictionary, sending each key and its value out the right outlet, then the dictionary out the
//! left.
//!
//! median's attributes hold one value, so `keys` (up to 256 symbols; `CLASS_ATTR_SYM_VARSIZE` in
//! the C) is registered with accessors written here. A key holding `::` or `[` (a path into the
//! dictionary) is resolved with `dictobj_key_parse`, which median-mock does not implement, so
//! paths are untested. Deviations: a string value is sent as a symbol, and an object value is
//! not sent (median's outlets refuse object atoms).

use std::{
    ffi::{c_long, c_void},
    sync::Mutex,
};

use median::{
    attr::{AttrBuilder, AttrType},
    dict::Ownership,
    max_sys::{self, e_max_errorcodes::*, t_max_err},
    prelude::*,
    sym,
};

const MAX_NUM_ITEMS: usize = 256;

#[derive(MaxObj)]
#[max(name = "median.dict.strip")]
pub struct DictStrip {
    obj: ObjRef,
    keys: Mutex<Vec<SymbolRef>>,
    dict_out: Outlet<Any>,
    keyvals: Outlet<Any>,
}

impl MaxObjWrapped for DictStrip {
    fn class_setup(class: &mut Class<MaxObjWrapper<Self>>) {
        // GAP: CLASS_ATTR_SYM_VARSIZE
        let mut b = AttrBuilder::new_accessors("keys", AttrType::SymbolRef, keys_get, keys_set);
        b.label(c"Keys").category(c"Dictionary");
        let added = b.build().map(|attr| class.add_attribute(attr));
        if !matches!(added, Ok(Ok(()))) {
            median::error!("dict.strip: could not add the keys attribute");
        }
    }

    fn new(builder: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        builder.inlet(Inlet::default().assist("dictionary input"));
        Ok(Self {
            obj: builder.obj(),
            keys: Mutex::new(symbols(builder.args().positional())),
            dict_out: builder
                .outlet(Outlet::any().assist("dictionary output stripped of specified keys")),
            keyvals: builder.outlet(
                Outlet::any().assist("series of key/value pairs stripped from the dictionary"),
            ),
        })
    }
}

#[median::methods]
impl DictStrip {
    #[dictionary]
    pub fn dictionary(&self, d: Dict) {
        let keys = self.keys.lock().unwrap().clone();
        for key in keys {
            let Some((target, entry)) = self.resolve(&d, key) else {
                median::object_error!(&self.obj, "could not parse key {key}");
                continue;
            };
            let Ok(Some(value)) = target.get(entry) else {
                continue;
            };
            // Kept until the entry is deleted, so a dictionary registered here stays findable
            // while its name is sent.
            let mut held = None;
            let atoms: Vec<Atom> = match value {
                DictValue::Int(i) => vec![i.into()],
                DictValue::Float(f) => vec![f.into()],
                DictValue::Symbol(s) => vec![s.into()],
                DictValue::Str(s) => vec![SymbolRef::from(s).into()],
                DictValue::Atoms(atoms) => atoms,
                DictValue::Dict(child) => {
                    let named = match child.name() {
                        Some(name) => Ok((child, name)),
                        None => child.register(None),
                    };
                    match named {
                        Ok((child, name)) => {
                            held = Some(child);
                            vec![sym!("dictionary").into(), name.into()]
                        }
                        Err(_) => Vec::new(),
                    }
                }
                DictValue::Object(_) => Vec::new(),
            };
            if !atoms.is_empty() {
                let _ = self.keyvals.send(&key, &atoms);
            }
            let _ = target.delete(entry);
            drop(held);
        }
        let _ = self.dict_out.send_dict(&d);
        let _ = d.notify_modified();
    }

    /// The dictionary holding `key` and the key's last part.
    fn resolve(&self, d: &Dict, key: SymbolRef) -> Option<(Dict, SymbolRef)> {
        let text = key.name().to_bytes();
        if !text.windows(2).any(|w| w == b"::") && !text.contains(&b'[') {
            return Some((d.clone(), key));
        }
        let mut akey = Atom::from(key);
        let mut target = std::ptr::null_mut();
        let mut last = std::ptr::null_mut();
        // SAFETY: the object and `d` are live, `akey` is a symbol atom (`Atom` is a transparent
        // `t_atom`) and the outputs are writable; the index is not asked for.
        // GAP: dictobj_key_parse
        let err = unsafe {
            max_sys::dictobj_key_parse(
                self.obj.as_ptr(),
                d.as_ptr(),
                (&raw mut akey).cast(),
                0,
                &mut target,
                &mut last,
                std::ptr::null_mut(),
            )
        };
        if err != 0 || target.is_null() {
            return None;
        }
        // SAFETY: `target` is `d` or a dictionary inside it, alive while `d` is held, and is
        // retained here. `last` is null or a symbol.
        let target = unsafe { Dict::from_raw(target, Ownership::Borrowed) }.ok()?;
        Some((target, unsafe { SymbolRef::from_raw(last) }))
    }
}

// Non-symbols become the empty symbol, as `atom_getsym` makes them.
fn symbols(atoms: &[Atom]) -> Vec<SymbolRef> {
    atoms
        .iter()
        .take(MAX_NUM_ITEMS)
        .map(Atom::get_symbol)
        .collect()
}

/// The `keys` getter: one symbol atom per key, in an array allocated as Max expects if the one
/// given is too small.
///
/// # Safety
/// Only Max calls it, as an attribute getter, with the `ac` and `av` it passes getters.
unsafe extern "C" fn keys_get(
    w: &MaxObjWrapper<DictStrip>,
    _attr: *mut c_void,
    ac: *mut c_long,
    av: *mut *mut max_sys::t_atom,
) -> t_max_err {
    median::ffi::guard_or(w, "keys getter", MAX_ERR_GENERIC as _, || {
        if ac.is_null() || av.is_null() {
            return MAX_ERR_NONE as _;
        }
        let keys = w.wrapped().keys.lock().unwrap().clone();
        // SAFETY: `ac` and `av` are the getter's out-parameters; `*av` is null or holds `*ac`
        // atoms, and a new array is the caller's to free with `sysmem_freeptr`.
        unsafe {
            if (*av).is_null() || (*ac as usize) < keys.len() {
                let size = keys.len().max(1) * std::mem::size_of::<max_sys::t_atom>();
                // GAP: sysmem_newptr
                let p = max_sys::sysmem_newptr(size as _).cast::<max_sys::t_atom>();
                if p.is_null() {
                    *ac = 0;
                    return MAX_ERR_OUT_OF_MEM as _;
                }
                *av = p;
            }
            *ac = keys.len() as c_long;
            for (i, key) in keys.into_iter().enumerate() {
                (*av).add(i).cast::<Atom>().write(Atom::from(key));
            }
        }
        MAX_ERR_NONE as _
    })
}

/// The `keys` setter.
///
/// # Safety
/// Only Max calls it, as an attribute setter, with `av` holding `ac` atoms.
unsafe extern "C" fn keys_set(
    w: &MaxObjWrapper<DictStrip>,
    _attr: *mut c_void,
    ac: c_long,
    av: *mut max_sys::t_atom,
) -> t_max_err {
    median::ffi::guard_or(w, "keys setter", MAX_ERR_GENERIC as _, || {
        let atoms: &[Atom] = if ac > 0 && !av.is_null() {
            // SAFETY: Max passes `ac` valid atoms; `Atom` is a transparent `t_atom`.
            unsafe { std::slice::from_raw_parts(av.cast(), ac as usize) }
        } else {
            &[]
        };
        *w.wrapped().keys.lock().unwrap() = symbols(atoms);
        MAX_ERR_NONE as _
    })
}

median::ext_main! { DictStrip }

#[cfg(test)]
mod tests {
    use median::{prelude::*, sym};
    use median_mock::Atom as MockAtom;

    fn sym_atoms(names: &[&str]) -> Vec<MockAtom> {
        names.iter().map(|s| MockAtom::Sym(s.to_string())).collect()
    }

    #[test]
    fn strips_keys() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object("median.dict.strip", &sym_atoms(&["a", "missing"]));
        assert!(!x.is_null());
        assert_eq!(k.attr_get(x, "keys").unwrap(), sym_atoms(&["a", "missing"]));
        k.attr_set(x, "keys", &sym_atoms(&["a", "b", "c", "missing"]))
            .unwrap();
        assert_eq!(k.attr_get(x, "keys").unwrap().len(), 4);

        let d = Dict::new().unwrap();
        d.insert(sym!("a"), 1i64).unwrap();
        d.insert(sym!("b"), &[Atom::from(2.5), Atom::from(sym!("x"))][..])
            .unwrap();
        d.insert(sym!("c"), Dict::new().unwrap()).unwrap();
        d.insert(sym!("d"), sym!("kept")).unwrap();
        let (d, name) = d.register(Some(sym!("src"))).unwrap();
        k.send(x, "dictionary", &[MockAtom::Sym(name.to_string())])
            .unwrap();
        assert!(k.take_console_log().is_empty());

        let out = k.take_outlet_log();
        assert_eq!(out.len(), 4);
        assert_eq!(
            (out[0].selector.as_str(), &out[0].atoms[..]),
            ("a", &[MockAtom::Long(1)][..])
        );
        assert_eq!(out[1].selector, "b");
        assert_eq!(
            out[1].atoms,
            [MockAtom::Float(2.5), MockAtom::Sym("x".into())]
        );
        assert_eq!(out[2].selector, "c");
        assert_eq!(out[2].atoms[0], MockAtom::Sym("dictionary".into()));
        assert_eq!(out[2].atoms.len(), 2);
        assert_eq!(
            (out[3].selector.as_str(), &out[3].atoms[..]),
            ("dictionary", &[MockAtom::Sym("src".into())][..])
        );
        let keys: Vec<String> = d.keys().unwrap().iter().map(|s| s.to_string()).collect();
        assert_eq!(keys, ["d"]);
        drop(d);
        assert!(k.free_object(x));
    }
}
