//! JSON/file subset for wrapper tests. Max dictionary syntax is modelled only for flat
//! `key: value` pairs; JSON arrays support scalar atoms, not nested arrays/objects.
use super::*;
use core::ffi::{c_int, c_short};
use serde_json::Value;

fn parse_dict(value: &Value) -> Result<usize, ()> {
    let Value::Object(entries) = value else {
        return Err(());
    };
    let p = alloc(Body::Dict(Vec::new()));
    let result = (|| {
        for (key, value) in entries {
            let entry = match value {
                Value::Object(_) => Entry::Dict(parse_dict(value)?),
                Value::Array(values) => Entry::Array(alloc(Body::Array(Atoms(
                    values.iter().map(scalar).collect::<Result<Vec<_>, _>>()?,
                )))),
                Value::String(s) => Entry::String(CString::new(s.as_str()).map_err(|_| ())?),
                _ => Entry::Scalar(Atom::from_raw(&scalar(value)?)),
            };
            append(p, intern(key.as_bytes()) as usize, entry);
        }
        Ok(p)
    })();
    if result.is_err() {
        release(p);
    }
    result
}
fn scalar(value: &Value) -> Result<t_atom, ()> {
    Ok(match value {
        Value::Bool(b) => Atom::Long(i64::from(*b)),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Atom::Long(i)
            } else {
                Atom::Float(n.as_f64().ok_or(())?)
            }
        }
        Value::String(s) if !s.contains('\0') => Atom::Sym(s.clone()),
        _ => return Err(()),
    }
    .to_raw())
}
fn atom_json(atom: &t_atom) -> Result<Value, ()> {
    match Atom::from_raw(atom) {
        Atom::Long(i) => Ok(i.into()),
        Atom::Float(f) => serde_json::Number::from_f64(f).map(Value::Number).ok_or(()),
        Atom::Sym(s) => Ok(s.into()),
        _ => Err(()),
    }
}
fn json(p: usize) -> Result<Value, ()> {
    let object = lookup(p);
    let lock = object.body.lock();
    let Body::Dict(entries) = &*lock.borrow() else {
        return Err(());
    };
    let mut result = serde_json::Map::new();
    for (key, value) in entries {
        result.insert(
            name_of(*key),
            match value {
                Entry::Scalar(a) => atom_json(&a.to_raw())?,
                Entry::String(s) => s.to_string_lossy().into_owned().into(),
                Entry::Dict(child) => json(*child)?,
                Entry::Array(array) => {
                    let a = lookup(*array);
                    let lock = a.body.lock();
                    let Body::Array(atoms) = &*lock.borrow() else {
                        return Err(());
                    };
                    Value::Array(atoms.0.iter().map(atom_json).collect::<Result<_, _>>()?)
                }
                Entry::Object(_, _) => return Err(()),
            },
        );
    }
    Ok(Value::Object(result))
}
// Failure fixtures test both null output and a returned partial dictionary on error.
static FAIL: Mutex<Option<bool>> = Mutex::new(None);
pub fn fail_next_constructor(after_allocation: bool) {
    *FAIL.lock() = Some(after_allocation);
}
pub(super) fn reset() {
    *FAIL.lock() = None;
}
unsafe fn construct(
    out: *mut *mut t_dictionary,
    f: impl FnOnce() -> Result<usize, ()>,
) -> t_max_err {
    assert!(unsafe { (*out).is_null() });
    let failure = FAIL.lock().take();
    if failure == Some(false) {
        return ERR;
    }
    match f() {
        Ok(p) => {
            unsafe {
                out.write(p as _);
            }
            if failure == Some(true) { ERR } else { 0 }
        }
        Err(()) => ERR,
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dictobj_dictionaryfromstring(
    out: *mut *mut t_dictionary,
    text: *const c_char,
    is_json: c_int,
    error: *mut c_char,
) -> t_max_err {
    let result = unsafe {
        construct(out, || {
            if is_json == 0 {
                return Err(());
            }
            let value: Value =
                serde_json::from_slice(CStr::from_ptr(text).to_bytes()).map_err(|_| ())?;
            parse_dict(&value)
        })
    };
    if result != 0 && !error.is_null() {
        unsafe {
            error.write(0);
        }
    }
    result
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dictobj_dictionaryfromatoms(
    out: *mut *mut t_dictionary,
    count: c_long,
    atoms: *const t_atom,
) -> t_max_err {
    unsafe {
        construct(out, || {
            if count < 0 || count % 2 != 0 {
                return Err(());
            }
            let atoms = if count == 0 {
                &[]
            } else {
                std::slice::from_raw_parts(atoms, count as usize)
            };
            let mut map = serde_json::Map::new();
            for pair in atoms.chunks_exact(2) {
                let Atom::Sym(key) = Atom::from_raw(&pair[0]) else {
                    return Err(());
                };
                let key = key.strip_suffix(':').ok_or(())?;
                map.insert(key.into(), atom_json(&pair[1])?);
            }
            parse_dict(&Value::Object(map))
        })
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dictionary_read(
    filename: *const c_char,
    path: c_short,
    out: *mut *mut t_dictionary,
) -> t_max_err {
    unsafe {
        construct(out, || {
            if path != 0 {
                return Err(());
            }
            let data = std::fs::read(CStr::from_ptr(filename).to_str().map_err(|_| ())?)
                .map_err(|_| ())?;
            parse_dict(&serde_json::from_slice(&data).map_err(|_| ())?)
        })
    }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dictionary_write(
    d: *mut t_dictionary,
    filename: *const c_char,
    path: c_short,
) -> t_max_err {
    if path != 0 {
        return ERR;
    }
    let result = (|| {
        let data = serde_json::to_vec(&json(d as usize)?).map_err(|_| ())?;
        std::fs::write(
            unsafe { CStr::from_ptr(filename) }
                .to_str()
                .map_err(|_| ())?,
            data,
        )
        .map_err(|_| ())
    })();
    if result.is_ok() { 0 } else { ERR }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn dictionary_dump(d: *mut t_dictionary, _: c_long, _: c_long) -> t_max_err {
    if json(d as usize).is_ok() { 0 } else { ERR }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsing_and_partial_failures_transfer_exactly_one_reference() {
        let _k = crate::lock();
        for failure in [None, Some(false), Some(true)] {
            let baseline = live_count();
            if let Some(partial) = failure {
                fail_next_constructor(partial);
            }
            let mut out = std::ptr::null_mut();
            let code = unsafe {
                dictobj_dictionaryfromstring(
                    &mut out,
                    c"{\"child\":{\"n\":2},\"array\":[1,3]}".as_ptr(),
                    1,
                    std::ptr::null_mut(),
                )
            };
            assert_eq!(code == 0, failure.is_none());
            if !out.is_null() {
                assert_eq!(json(out as usize).unwrap()["child"]["n"], 2);
                release(out as usize);
            }
            assert_eq!(live_count(), baseline);
        }
        let baseline = live_count();
        assert!(parse_dict(&serde_json::json!({"child": {"n": 2}, "invalid": null})).is_err());
        assert_eq!(live_count(), baseline);
    }
}
