use median::{
    attr::{AttrBuilder, AttrType},
    builder::MaxWrappedBuilder,
    class::Class,
    clock::ClockHandle,
    inlet::MaxInlet,
    num::{Float, Int},
    object::MaxObj,
    outlet::OutList,
    post,
    symbol::SymbolRef,
    wrapper::{
        attr_get_tramp, attr_set_tramp, tramp, MaxObjWrapped, MaxObjWrapper, WrapperWrapped,
    },
};

use std::convert::{From, TryFrom};

use std::ffi::c_void;
use std::os::raw::c_long;

median::external! {
    //#[name="simp"]
    pub struct Simp {
        pub value: Int,
        pub fvalue: Float,
        _v: String,
        clock: ClockHandle,
        list_out: OutList,
    }

    type Wrapper = MaxObjWrapper<Simp>;

    impl MaxObjWrapped<Simp> for Simp {
        fn new(builder: &mut dyn MaxWrappedBuilder<Self>) -> Self {
            //can call closure
            builder.add_inlet(MaxInlet::Float(Box::new(|_s, v| {
                post!("got float {}", v);
            })));
            //also can call method
            builder.add_inlet(MaxInlet::Int(Box::new(Self::int)));
            let _ = builder.add_inlet(MaxInlet::Proxy);
            Self {
                value: Int::new(0),
                fvalue: Default::default(),
                _v: String::from("blah"),
                clock: builder.with_clockfn(Self::clocked),
                list_out: builder.add_list_outlet(),
            }
        }

        /// Register any methods you need for your class
        fn class_setup(c: &mut Class<MaxObjWrapper<Self>>) {
            pub extern "C" fn attr_get_trampoline(
                w: &MaxObjWrapper<Simp>,
                _attr: c_void,
                ac: *mut c_long,
                av: *mut *mut max_sys::t_atom,
            ) {
                median::attr::get(ac, av, || w.wrapped().value.get());
            }

            pub extern "C" fn attr_set_trampoline(
                w: &MaxObjWrapper<Simp>,
                _attr: c_void,
                ac: c_long,
                av: *mut max_sys::t_atom,
            ) {
                median::attr::set(ac, av, |v: i64| w.wrapped().value.set(v));
            }

            c.add_method(median::method::Method::Int(Self::int_tramp));
            c.add_method(median::method::Method::Bang(Self::bang_tramp));

            c.add_attribute(
                AttrBuilder::new_accessors(
                    "blah",
                    AttrType::Int64,
                    attr_get_trampoline,
                    attr_set_trampoline,
                )
                .build()
                .unwrap(),
            )
                .expect("failed to add attribute");

            c.add_attribute(
                AttrBuilder::new_accessors(
                    "foo",
                    AttrType::Float64,
                    Self::foo_tramp,
                    Self::set_foo_tramp,
                )
                .build()
                .unwrap(),
            )
                .expect("failed to add attribute");
            }
    }

    impl Simp {
        #[tramp(Wrapper)]
        pub fn bang(&self) {
            let i = median::inlet::Proxy::get_inlet(self.max_obj());
            post!("from rust {} inlet {}", self.value, i);
            self.clock.delay(10);
        }

        #[tramp(Wrapper)]
        pub fn int(&self, v: i64) {
            let i = median::inlet::Proxy::get_inlet(self.max_obj());
            self.value.set(v);
            median::attr::touch_with_name(self.max_obj(), SymbolRef::try_from("blah").unwrap())
                .unwrap();
            //XXX won't compile, needs mutex
            //self._v = format!("from rust {}", self.value);
            post!("from rust {} inlet {}", self.value, i);
        }

        #[attr_get_tramp(Wrapper)]
        pub fn foo(&self) -> f64 {
            self.fvalue.get()
        }

        #[attr_set_tramp(Wrapper)]
        pub fn set_foo(&self, v: f64) {
            self.fvalue.set(v);
        }

        pub fn clocked(&self) {
            post("clocked".to_string());
            let _ = self.list_out.send(&[
                1i64.into(),
                12f64.into(),
                SymbolRef::try_from("foo").unwrap().into(),
            ]);
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn ext_main(_r: *mut c_void) {
    MaxObjWrapper::<Simp>::register(false)
}
