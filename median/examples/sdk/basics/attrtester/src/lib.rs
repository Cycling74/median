//! A port of the SDK's `basics/attrtester`: an int clipped by attributes, showing attribute
//! options (clip, label, style, category, order, enumeration, hidden, save, custom accessors).
//!
//! `usemin` and `usemax` are `char` attributes in the SDK; median has no `char` field attribute,
//! so they are `long`s with the `onoff` style.

use median::prelude::*;

#[derive(MaxObj)]
#[max(name = "median.attrtester")]
pub struct AttrTester {
    #[attr(
        category = "Value",
        order = 1,
        enum = "zero one two \"three four\" five",
        label = "Enumerated Value",
        hidden
    )]
    val: Int64,
    #[attr(style = "onoff", label = "Use Minimum")]
    usemin: Int64,
    #[attr(style = "onoff", label = "Use Maximum", order = -1)]
    usemax: Int64,
    #[attr(clip(0, 100))]
    max: Int64,
    min_value: Int64,
    out: Outlet<Int>,
}

impl MaxObjWrapped for AttrTester {
    fn class_setup(_class: &mut Class<MaxObjWrapper<Self>>) {
        post!("I am the attrtester object");
    }

    fn new(builder: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        builder.inlet(Inlet::default().assist("I am inlet 0"));
        Ok(Self {
            val: Int64::new(0),
            usemin: Int64::new(0),
            usemax: Int64::new(0),
            max: Int64::new(0),
            min_value: Int64::new(-52),
            out: builder.outlet(Outlet::int().assist("I am outlet 0")),
        })
    }
}

#[median::methods]
impl AttrTester {
    #[int]
    pub fn int(&self, mut n: t_atom_long) {
        if self.usemin.get() != 0 {
            n = n.max(self.min_value.get());
        }
        if self.usemax.get() != 0 {
            n = n.min(self.max.get());
        }
        self.val.set(n);
        self.bang();
    }

    #[float]
    pub fn float(&self, f: f64) {
        self.int(f as t_atom_long);
    }

    #[bang]
    pub fn bang(&self) {
        let _ = self.out.send(self.val.get());
    }

    #[attr(name = "min", save)]
    pub fn min(&self) -> t_atom_long {
        self.min_value.get()
    }

    // Refuses a minimum above the maximum, silently.
    pub fn set_min(&self, min: t_atom_long) {
        if min <= self.max.get() {
            self.min_value.set(min);
        }
    }
}

median::ext_main! { AttrTester }

#[cfg(test)]
mod tests {
    use median_mock::Atom;

    #[test]
    fn clips_by_the_attributes() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object(
            "median.attrtester",
            &[
                Atom::Sym("@max".into()),
                Atom::Long(200),
                Atom::Sym("@usemax".into()),
                Atom::Long(1),
            ],
        );
        assert!(!x.is_null());
        assert_eq!(k.attr_get(x, "max").unwrap(), [Atom::Long(100)]);
        assert_eq!(k.attr_get(x, "min").unwrap(), [Atom::Long(-52)]);
        k.send_int(x, 150).unwrap();
        k.attr_set(x, "usemin", &[Atom::Long(1)]).unwrap();
        k.send_float(x, -60.7).unwrap();
        // Above the maximum: refused.
        k.attr_set(x, "min", &[Atom::Long(101)]).unwrap();
        k.attr_set(x, "min", &[Atom::Long(10)]).unwrap();
        k.send_int(x, 3).unwrap();
        k.send_bang(x).unwrap();
        let out: Vec<_> = k.take_outlet_log().into_iter().map(|e| e.atoms).collect();
        assert_eq!(
            out,
            [
                vec![Atom::Long(100)],
                vec![Atom::Long(-52)],
                vec![Atom::Long(10)],
                vec![Atom::Long(10)]
            ]
        );
        assert_eq!(k.attr_get(x, "val").unwrap(), [Atom::Long(10)]);
        assert_eq!(k.assist(x, true, 0).as_deref(), Some("I am outlet 0"));
        assert!(k.free_object(x));
    }

    #[test]
    fn attribute_options() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object("median.attrtester", &[]);
        let attr = |a: &str, n: &str| k.attr_attr(x, a, n).map(|(_, atoms)| atoms);
        assert_eq!(attr("val", "invisible"), Some(vec![Atom::Long(1)]));
        assert_eq!(
            attr("val", "style"),
            Some(vec![Atom::Sym("enumindex".into())])
        );
        assert_eq!(attr("val", "order"), Some(vec![Atom::Long(1)]));
        assert_eq!(
            attr("val", "category"),
            Some(vec![Atom::Sym("Value".into())])
        );
        assert_eq!(
            attr("val", "label"),
            Some(vec![Atom::Sym("Enumerated Value".into())])
        );
        assert_eq!(attr("val", "enumvals").map(|v| v.len()), Some(5));
        assert_eq!(
            attr("usemin", "style"),
            Some(vec![Atom::Sym("onoff".into())])
        );
        assert_eq!(attr("usemax", "order"), Some(vec![Atom::Long(-1)]));
        assert_eq!(attr("min", "save"), Some(vec![Atom::Long(1)]));
        assert!(k.free_object(x));
    }
}
