//! A port of the SDK's `advanced/linky`: appends atoms to a list, deletes by index, posts and
//! reverses it.
//!
//! The C demonstrates Max's `t_linklist`; a `Vec` stands in for it here. Its outlet is never
//! used, as in the C.

use std::sync::Mutex;

use median::{atom::AtomValue, prelude::*};

// `Atom` is not `Send`, so the list holds these instead.
#[derive(Clone, Copy)]
enum Item {
    Int(t_atom_long),
    Float(f64),
    Sym(SymbolRef),
    Other,
}

impl From<&Atom> for Item {
    fn from(a: &Atom) -> Self {
        match a.get_value() {
            Some(AtomValue::Int(i)) => Item::Int(i),
            Some(AtomValue::Float(f)) => Item::Float(f),
            Some(AtomValue::Symbol(s)) => Item::Sym(s),
            _ => Item::Other,
        }
    }
}

#[derive(MaxObj)]
#[max(name = "median.linky")]
pub struct Linky {
    obj: ObjRef,
    list: Mutex<Vec<Item>>,
    _out: Outlet<Any>,
}

impl MaxObjWrapped for Linky {
    fn new(builder: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        Ok(Self {
            obj: builder.obj(),
            list: Mutex::new(Vec::new()),
            _out: builder.outlet(Outlet::any()),
        })
    }
}

#[median::methods]
impl Linky {
    #[gimme("append")]
    pub fn append(&self, atoms: &[Atom]) {
        self.list
            .lock()
            .unwrap()
            .extend(atoms.iter().map(Item::from));
    }

    #[sel("deleteindex")]
    pub fn deleteindex(&self, index: t_atom_long) {
        let mut list = self.list.lock().unwrap();
        match usize::try_from(index) {
            Ok(i) if i < list.len() => {
                list.remove(i);
            }
            _ => self.obj.error("deleteindex failed"),
        }
    }

    #[sel("post")]
    pub fn post(&self) {
        for item in self.list.lock().unwrap().iter() {
            match item {
                Item::Int(i) => self.obj.post(format!("post: {i}")),
                Item::Float(f) => self.obj.post(format!("post: {f:.6}")),
                Item::Sym(s) => self.obj.post(format!("post: {s}")),
                Item::Other => self.obj.post("cannot post -- unknown type"),
            }
        }
    }

    #[sel("reverse")]
    pub fn reverse(&self) {
        self.list.lock().unwrap().reverse();
    }
}

median::ext_main! { Linky }

#[cfg(test)]
mod tests {
    use median_mock::Atom;

    fn posted(k: &median_mock::Kernel) -> Vec<String> {
        k.take_console_log().into_iter().map(|e| e.text).collect()
    }

    #[test]
    fn appends_deletes_reverses_and_posts() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object("median.linky", &[]);
        assert!(!x.is_null());
        k.send(
            x,
            "append",
            &[Atom::Long(1), Atom::Float(2.5), Atom::Sym("three".into())],
        )
        .unwrap();
        k.send(x, "append", &[Atom::Long(4)]).unwrap();
        k.send(x, "post", &[]).unwrap();
        assert_eq!(
            posted(&k),
            ["post: 1", "post: 2.500000", "post: three", "post: 4"]
        );
        k.send(x, "deleteindex", &[Atom::Long(1)]).unwrap();
        k.send(x, "reverse", &[]).unwrap();
        k.send(x, "post", &[]).unwrap();
        assert_eq!(posted(&k), ["post: 4", "post: three", "post: 1"]);
        k.send(x, "deleteindex", &[Atom::Long(3)]).unwrap();
        k.send(x, "deleteindex", &[Atom::Long(-1)]).unwrap();
        assert_eq!(posted(&k), ["deleteindex failed", "deleteindex failed"]);
        assert!(k.take_outlet_log().is_empty());
        assert!(k.free_object(x));
    }
}
