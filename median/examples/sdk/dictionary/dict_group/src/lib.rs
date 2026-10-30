//! A port of the SDK's `dictionary/dict.group`: collects key/value messages into a named
//! dictionary; bang sends it and starts a fresh one.

use std::sync::Mutex;

use median::{atom::AtomValue, dict::Dict, prelude::*};

#[derive(MaxObj)]
#[max(name = "median.dict.group")]
pub struct DictGroup {
    obj: ObjRef,
    dict: Mutex<Dict>,
    out: Outlet<Any>,
}

impl MaxObjWrapped for DictGroup {
    fn new(builder: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        builder.inlet(Inlet::default().assist(
            "dictionary syntax for appending a key/value pair, bang sends the dictionary and resets",
        ));
        let name: Option<SymbolRef> = builder.args().at(0).get()?;
        // Without a name argument the dictionary gets a unique one, as `symbol_unique` would;
        // an `@name` argument re-registers it after `new`.
        let (dict, _) = Dict::new()
            .map_err(|e| e.to_string())?
            .register(name)
            // `RegisterError` has no `Display`.
            .map_err(|e| format!("could not register the dictionary: {e:?}"))?;
        Ok(Self {
            obj: builder.obj(),
            dict: Mutex::new(dict),
            out: builder.outlet(Outlet::any().assist("dictionary output")),
        })
    }
}

#[median::methods]
impl DictGroup {
    #[bang]
    pub fn bang(&self) {
        let dict = self.dict.lock().unwrap();
        let _ = self.out.send_dict(&dict);
        let _ = dict.clear();
    }

    #[any]
    pub fn anything(&self, sel: &SymbolRef, atoms: &[Atom]) {
        let explicit_colon = sel.to_string().contains(':')
            || atoms.first().is_some_and(|a| match a.get_value() {
                Some(AtomValue::Symbol(s)) => s.to_string().starts_with(':'),
                _ => false,
            });
        // `key value ...` is parsed as `key : value ...`, the dictionary syntax.
        let mut parse = vec![Atom::from(*sel)];
        if !explicit_colon {
            parse.push(Atom::from(median::sym!(":")));
        }
        parse.extend(atoms.iter().map(copy_atom));
        let Ok(parsed) = Dict::from_atoms(&parse) else {
            median::object_error!(&self.obj, "could not create dictionary from args");
            return;
        };
        let dict = self.dict.lock().unwrap();
        for key in parsed.keys().unwrap_or_default() {
            let _ = dict.delete(key);
            match parsed.get(key) {
                Ok(Some(value)) => {
                    let _ = dict.insert(key, value);
                }
                _ => median::object_error!(&self.obj, "unknown atom type"),
            }
        }
        // The SDK returns before notifying when it parses: subscribers are not told here either.
    }

    #[list]
    pub fn list(&self, atoms: &[Atom]) {
        let Some((first, rest)) = atoms.split_first() else {
            return;
        };
        let key = match first.get_value() {
            Some(AtomValue::Symbol(s)) => Ok(s),
            Some(AtomValue::Int(i)) => SymbolRef::try_from(i.to_string()),
            Some(AtomValue::Float(f)) => SymbolRef::try_from(format!("{f:.6}")),
            _ => {
                median::object_error!(
                    &self.obj,
                    "unknown atom type received as first element of list"
                );
                return;
            }
        };
        if let Ok(key) = key {
            self.anything(&key, rest);
        }
    }

    #[attr(label = "Name", category = "Dictionary")]
    pub fn name(&self) -> SymbolRef {
        self.dict.lock().unwrap().name().unwrap_or_default()
    }

    pub fn set_name(&self, name: SymbolRef) {
        let mut dict = self.dict.lock().unwrap();
        match dict.rename(name) {
            Ok(renamed) => *dict = renamed,
            Err(_) => {
                median::object_error!(&self.obj, "could not create dictionary named {}", name)
            }
        }
    }
}

fn copy_atom(a: &Atom) -> Atom {
    match a.get_value() {
        Some(AtomValue::Int(i)) => i.into(),
        Some(AtomValue::Float(f)) => f.into(),
        _ => a.get_symbol().into(),
    }
}

median::ext_main! { DictGroup }

#[cfg(test)]
mod tests {
    use median_mock::Atom;

    #[test]
    fn groups_pairs_and_resets_on_bang() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object("median.dict.group", &[Atom::Sym("grp".into())]);
        assert!(!x.is_null());
        assert_eq!(k.attr_get(x, "name").unwrap(), [Atom::Sym("grp".into())]);
        // median-mock parses only `key: value` pairs with one value each, so the implicit-colon
        // and multi-value forms (`a 1`, `b : 2.5 3`) are untested here.
        k.send(x, "a:", &[Atom::Long(1)]).unwrap();
        k.send(x, "b:", &[Atom::Float(2.5)]).unwrap();
        k.send_list(x, &[Atom::Sym("c:".into()), Atom::Sym("seven".into())])
            .unwrap();
        k.send(x, "a:", &[Atom::Long(4)]).unwrap();
        assert!(k.take_console_log().is_empty());
        let d = median::dict::Dict::find_registered(median::sym!("grp")).unwrap();
        let keys: Vec<String> = d.keys().unwrap().iter().map(|s| s.to_string()).collect();
        assert_eq!(keys.len(), 3);
        assert!(matches!(
            d.get(median::sym!("a")),
            Ok(Some(median::dict::DictValue::Int(4)))
        ));
        k.send_bang(x).unwrap();
        let out = k.take_outlet_log();
        assert_eq!(out[0].selector, "dictionary");
        assert_eq!(out[0].atoms, [Atom::Sym("grp".into())]);
        assert!(d.is_empty());
        drop(d);
        assert!(k.free_object(x));
    }

    #[test]
    fn unnamed_gets_a_unique_name() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object("median.dict.group", &[]);
        let [Atom::Sym(name)] = &k.attr_get(x, "name").unwrap()[..] else {
            panic!("name is one symbol");
        };
        assert!(!name.is_empty());
        k.attr_set(x, "name", &[Atom::Sym("other".into())]).unwrap();
        assert_eq!(k.attr_get(x, "name").unwrap(), [Atom::Sym("other".into())]);
        assert!(k.free_object(x));
    }
}
