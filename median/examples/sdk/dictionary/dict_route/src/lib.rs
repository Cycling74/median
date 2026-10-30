//! A port of the SDK's `dictionary/dict.route`: sends a dictionary out the left outlet if it
//! matches a schema dictionary and out the right if not; a dictionary in the right inlet becomes
//! the schema.
//!
//! The schema is replaced with `Dict::clear` and `Dict::copy_to` rather than
//! `dictionary_clone_to_existing`, so a dictionary holding an array of dictionaries cannot become
//! the schema (an error is posted). Matching has no median API and median-mock has no
//! `dictobj_validate`, so routing is untested.

use median::{dict::DictResult, max_sys, prelude::*};

#[derive(MaxObj)]
#[max(name = "median.dict.route")]
pub struct DictRoute {
    obj: ObjRef,
    schema: Dict,
    matched: Outlet<Any>,
    unmatched: Outlet<Any>,
}

impl MaxObjWrapped for DictRoute {
    fn new(builder: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        // As in the C, whose per-inlet texts are overwritten, both inlets say this.
        builder.inlet(Inlet::default().assist("dictionary input"));
        builder.inlet(Inlet::proxy().assist("dictionary input"));
        let args = builder.args().positional();
        let schema = match Some(args).filter(|a| !a.is_empty()).map(Dict::from_atoms) {
            Some(Ok(d)) => d,
            _ => {
                Dict::from_json(r#"{ "schema" : "*" }"#).map_err(|e| format!("dict.route: {e}"))?
            }
        };
        let (schema, _) = schema
            .register(None)
            // `RegisterError` has no `Display`.
            .map_err(|e| format!("could not register the schema: {e:?}"))?;
        Ok(Self {
            obj: builder.obj(),
            schema,
            matched: builder
                .outlet(Outlet::any().assist("dictionary matching specified keys/values")),
            unmatched: builder
                .outlet(Outlet::any().assist("dictionary not-matching specified keys/values")),
        })
    }
}

#[median::methods]
impl DictRoute {
    #[dictionary]
    pub fn dictionary(&self, d: Dict) -> DictResult<()> {
        if self.obj.proxy_inlet() == 0 {
            // SAFETY: both dictionaries are live, held by `self.schema` and `d`. As in the C,
            // neither is locked.
            // GAP: dictobj_validate
            let valid = unsafe { max_sys::dictobj_validate(self.schema.as_ptr(), d.as_ptr()) };
            let out = if valid != 0 {
                &self.matched
            } else {
                &self.unmatched
            };
            let _ = out.send_dict(&d);
        } else {
            self.schema.clear()?;
            d.copy_to(&self.schema)?;
        }
        Ok(())
    }
}

median::ext_main! { DictRoute }

#[cfg(test)]
mod tests {
    use median::{prelude::*, sym};
    use median_mock::Atom as MockAtom;

    #[test]
    fn schema_inlet_and_errors() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object(
            "median.dict.route",
            &[MockAtom::Sym("a:".into()), MockAtom::Long(1)],
        );
        assert!(!x.is_null());
        let source = Dict::from_atoms(&[Atom::from(sym!("b:")), Atom::from(2i64)]).unwrap();
        let (source, name) = source.register(Some(sym!("source"))).unwrap();
        k.send_inlet(x, 1, "dictionary", &[MockAtom::Sym(name.to_string())])
            .unwrap();
        assert!(k.take_console_log().is_empty());
        k.send(x, "dictionary", &[MockAtom::Sym("missing".into())])
            .unwrap();
        let errors: Vec<_> = k.take_console_log().into_iter().map(|e| e.text).collect();
        assert_eq!(errors, ["unable to reference dictionary named missing"]);
        assert!(k.take_outlet_log().is_empty());
        assert_eq!(k.assist(x, false, 1).as_deref(), Some("dictionary input"));
        assert_eq!(
            k.assist(x, true, 1).as_deref(),
            Some("dictionary not-matching specified keys/values")
        );
        drop(source);
        assert!(k.free_object(x));
    }

    #[test]
    fn default_schema() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object("median.dict.route", &[]);
        assert!(!x.is_null());
        assert!(k.take_console_log().is_empty());
        assert!(k.free_object(x));
    }
}
