//! A port of the SDK's `basics/simplemax`: creation arguments and inlet assistance.

use median::{atom::AtomValue, prelude::*};

// This object has no state or message methods. Its ObjRef is only needed during construction.
#[derive(MaxObj)]
#[max(name = "simplemax", no_methods)]
pub struct SimpleMax;

impl MaxObjWrapped for SimpleMax {
    fn class_setup(_class: &mut Class<MaxObjWrapper<Self>>) {
        post!("I am the simplemax object");
    }

    fn new(builder: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        builder.inlet(Inlet::default().assist("I am inlet 0"));
        let obj = builder.obj();
        median::object_post!(&obj, "a new simplemax object was instantiated");
        //the whole list, `@name` atoms included: `raw()` is `creation_args()`
        let args = builder.args().raw();
        median::object_post!(&obj, "it has {} arguments", args.len());
        for (i, arg) in args.iter().enumerate() {
            match arg.get_value() {
                Some(AtomValue::Int(v)) => median::object_post!(&obj, "arg {i}: long ({v})"),
                Some(AtomValue::Float(v)) => median::object_post!(&obj, "arg {i}: float ({v:.6})"),
                Some(AtomValue::Symbol(v)) => median::object_post!(&obj, "arg {i}: symbol ({v})"),
                _ => median::object_error!(&obj, "forbidden argument"),
            }
        }
        Ok(Self)
    }
}

median::ext_main! { SimpleMax }
