//! A port of the SDK's `basics/simplejs`: a `nobox` class for `js` (through `maxclasswrap`), with
//! a `myattr` attribute, a `print` method and a `doAbs` method that answers its caller.
//!
//! `doAbs` replies as the SDK does, with a one-float atomarray, which `js` leaks when it
//! discards the reply.

use median::{atom::AtomValue, gimmeback::Reply, max_sys, prelude::*};

#[derive(MaxObj)]
#[max(name = "median.simplejs", class_type = nobox)]
pub struct SimpleJs {
    #[attr]
    myattr: Float64,
}

impl MaxObjWrapped for SimpleJs {
    fn class_setup(class: &mut Class<MaxObjWrapper<Self>>) {
        // GAP: CLASS_FLAG_POLYGLOT.
        // SAFETY: the class is live and not registered yet; nothing else touches its flags now.
        unsafe {
            (*class.inner()).c_flags |=
                max_sys::e_max_class_flags::CLASS_FLAG_POLYGLOT as std::ffi::c_long;
        }
    }

    fn new(_builder: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        Ok(Self {
            myattr: Float64::new(74.),
        })
    }
}

#[median::methods]
impl SimpleJs {
    #[sel("print")]
    pub fn print(&self) {
        post!("The value of myattr is: {:.6}", self.myattr.get());
    }

    #[gimmeback("doAbs")]
    pub fn do_abs(&self, atoms: &[Atom]) -> Reply {
        let f = match atoms.first().map(Atom::get_value) {
            Some(Some(AtomValue::Int(i))) => (i as f64).abs(),
            Some(Some(AtomValue::Float(f))) => f.abs(),
            Some(_) => 0.,
            None => {
                median::error("missing argument for method doAbs()");
                0.
            }
        };
        Reply::Atoms(vec![f.into()])
    }
}

median::ext_main! { SimpleJs }

#[cfg(test)]
mod tests {
    use median_mock::{Atom, CallReply};

    #[test]
    fn attribute_print_and_do_abs() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object("median.simplejs", &[]);
        assert!(!x.is_null());
        k.send(x, "print", &[]).unwrap();
        k.attr_set(x, "myattr", &[Atom::Float(2.5)]).unwrap();
        k.send(x, "print", &[]).unwrap();
        let console: Vec<_> = k.take_console_log().into_iter().map(|e| e.text).collect();
        assert_eq!(
            console,
            [
                "The value of myattr is: 74.000000",
                "The value of myattr is: 2.500000"
            ]
        );
        let abs = |atoms: &[Atom]| match k.call(x, "doAbs", atoms).unwrap() {
            CallReply::Atoms(a) => a,
            r => panic!("unexpected reply {r:?}"),
        };
        assert_eq!(abs(&[Atom::Long(-3)]), [Atom::Float(3.)]);
        assert_eq!(abs(&[Atom::Float(-0.5), Atom::Long(9)]), [Atom::Float(0.5)]);
        assert_eq!(abs(&[]), [Atom::Float(0.)]);
        assert_eq!(
            k.take_console_log()[0].text,
            "missing argument for method doAbs()"
        );
        assert!(k.free_object(x));
    }

    #[test]
    fn is_a_polyglot_nobox_class() {
        let _k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        use median::max_sys;
        // SAFETY: interned symbols; the class, if found, is live.
        let flags = unsafe {
            let c = max_sys::class_findbyname(
                max_sys::gensym(c"nobox".as_ptr()),
                max_sys::gensym(c"median.simplejs".as_ptr()),
            );
            assert!(!c.is_null());
            (*c).c_flags
        };
        assert_ne!(
            flags & max_sys::e_max_class_flags::CLASS_FLAG_POLYGLOT as std::ffi::c_long,
            0
        );
    }
}
