use median::{
    atom::Atom,
    attr::{AttrBuilder, AttrType},
    builder::MaxWrappedBuilder,
    class::Class,
    clock::ClockHandle,
    inlet::MaxInlet,
    max_sys::t_atom_long,
    num::{Float64, Int64},
    object::MaxObj,
    outlet::OutList,
    post,
    symbol::SymbolRef,
    wrapper::{attr_get_tramp, attr_set_tramp, MaxObjWrapped, MaxObjWrapper},
};

use std::convert::{From, TryFrom};

//you need to wrap your external in this macro to get the system to register your object and
//automatically generate trampolines and what not.
median::external! {
    //#[name="simp"]
    pub struct Simp {
        value: Int64,
        fvalue: Float64,
        _v: String,
        clock: ClockHandle,
        list_out: OutList,
    }

    //implement the max object wrapper
    impl MaxObjWrapped<Simp> for Simp {
        //create an instance of your object
        //setup inlets/outlets and clocks
        fn new(builder: &mut dyn MaxWrappedBuilder<Self>) -> Self {
            //can call closure
            builder.add_inlet(MaxInlet::Float(Box::new(|_s, v| {
                post!("got float {}", v);
            })));
            //also can call method
            builder.add_inlet(MaxInlet::Int(Box::new(Self::int)));
            let _ = builder.add_inlet(MaxInlet::Proxy);
            Self {
                value: Default::default(),
                fvalue: Default::default(),
                _v: String::from("blah"),
                clock: builder.with_clockfn(Self::clocked),
                list_out: builder.add_list_outlet(),
            }
        }

        // Register any methods you need for your class
        fn class_setup(c: &mut Class<MaxObjWrapper<Self>>) {

            //register a attribute "foo" with the given type, getter and setter
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

            //register a attribute "blah" with the given type, getter and setter
            c.add_attribute(
                AttrBuilder::new_accessors(
                    "blah",
                    AttrType::Int64,
                    Self::blah_tramp,
                    Self::set_blah_tramp,
                )
                .build()
                .unwrap(),
            )
                .expect("failed to add attribute");
        }
    }

    //implement any methods you might want for your object that aren't part of the wrapper
    impl Simp {
        //create a "bang" method and automatically register it.
        //the name of the method can be anything you want
        #[bang]
        pub fn bang(&self) {
            let i = median::inlet::Proxy::get_inlet(self.max_obj());
            median::object_post!(self.max_obj(), "from rust {} inlet {}", self.value, i);
            self.clock.delay(10);
        }

        //create an "int" method and automatically register it.
        //the name of the method can be anything you want, it must take 1 argument other than &self
        //that is of type t_atom_long
        #[int]
        pub fn int(&self, v: t_atom_long) {
            let i = median::inlet::Proxy::get_inlet(self.max_obj());
            self.value.set(v);
            median::attr::touch_with_name(self.max_obj(), SymbolRef::try_from("blah").unwrap())
                .unwrap();
            //XXX won't compile, needs mutex
            //self._v = format!("from rust {}", self.value);
            post!("from rust {} inlet {}", self.value, i);
        }

        //create a "list" method and automatically register it
        #[list]
        pub fn list(&self, atoms: &[Atom]) {
            post!("got list with length {}", atoms.len());
        }

        //create anl "any" method and automatically register it
        #[any]
        pub fn baz(&self, sel: &SymbolRef, atoms: &[Atom]) {
            post!("got any with sel {} and length {}", sel, atoms.len());
        }

        //create a float attribute getter trampoline (see registration in class setup)
        #[attr_get_tramp]
        pub fn foo(&self) -> f64 {
            self.fvalue.get()
        }

        //create a float attribute setter trampoline (see registration in class setup)
        #[attr_set_tramp]
        pub fn set_foo(&self, v: f64) {
            self.fvalue.set(v);
        }

        //create a long attribute getter trampoline (see registration in class setup)
        #[attr_get_tramp]
        pub fn blah(&self) -> t_atom_long {
            self.value.get()
        }

        //create a long attribute setter trampoline (see registration in class setup)
        #[attr_set_tramp]
        pub fn set_blah(&self, v: t_atom_long) {
            self.value.set(v);
        }

        //called via the `clock` member (see the `bang` method and `new` above)
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
