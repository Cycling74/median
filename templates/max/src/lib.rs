use median::{
    attr::{AttrBuilder, AttrType},
    builder::MaxWrappedBuilder,
    class::Class,
    max_sys::t_atom_long,
    num::Float64,
    object::MaxObj,
    post,
    wrapper::{attr_get_tramp, attr_set_tramp, MaxObjWrapped, MaxObjWrapper},
};

use std::convert::{From, TryFrom};

//you need to wrap your external in this macro to get the system to register your object and
//automatically generate trampolines and what not.
median::external! {
    #[name="{{crate_name}}"]
    pub struct MaxExtern { }

    //implement the max object wrapper
    impl MaxObjWrapped<MaxExtern> for MaxExtern {
        //create an instance of your object
        //setup inlets/outlets and clocks
        fn new(_builder: &mut dyn MaxWrappedBuilder<Self>) -> Self {
            Self { }
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
        }
    }

    //implement any methods you might want for your object that aren't part of the wrapper
    impl MaxExtern {
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
            post!("from rust {} inlet {}", v, i);
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
    }
}
