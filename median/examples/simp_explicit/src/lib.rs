//The `simp` example written without `#[derive(MaxObject)]` and `#[median::methods]`: every impl
//the macros generate is written out here, and they are short. This is a supported way to write
//an external, not a fallback: it is what to read to see what the macros do, and the way to
//register things conditionally or under computed names. The object behaves exactly like `simp`.

use median::{atom::AtomValue, prelude::*};
use std::ffi::CStr;

pub struct SimpExplicit {
    obj: ObjRef,
    value: Int64,
    fvalue: Float64,
    _v: String,
    clock: Clock,
    changed: Queue,
    list_out: Outlet<List>,
}

//the class identity (what `#[derive(MaxObject)]` and `#[max(name = "simp_explicit")]` give)
impl ObjClass for SimpExplicit {
    //the wrapper Max allocates: `MSPObjWrapper<Self>` for an MSP object
    type Wrapper = MaxObjWrapper<Self>;

    fn class_name() -> &'static CStr {
        c"simp_explicit"
    }
}

impl SimpExplicit {
    //the derive generates this too: `ext_main!` below calls it
    pub fn register() {
        MaxObjWrapper::<Self>::register()
    }
}

//attributes backed by fields (what `#[attr(clip(0, 100), label = "Value", category = "Simp",
//save)]` on `value` gives)
impl FieldAttrs for SimpExplicit {
    fn register_attrs<W>(c: &mut Class<W>) -> MaxResult<()>
    where
        W: WrapperWrapped<Self> + MaxObj,
    {
        c.add_field_attr(
            c"value",
            |s: &Self| &s.value,
            AttrOptions::new()
                .clip(0., 100.)
                .label(c"Value")
                .category(c"Simp")
                .save(),
        )
    }
}

//the message methods and accessor attributes (what `#[median::methods]` gives). Each `add_*`
//takes the method itself; median builds the function Max calls from its type.
impl Methods for SimpExplicit {
    fn register<W>(c: &mut Class<W>) -> MaxResult<()>
    where
        W: WrapperWrapped<Self> + MaxObj,
    {
        c.add_bang(Self::bang)?;
        c.add_int(Self::int)?;
        c.add_list(Self::list)?;
        c.add_any(Self::baz)?;
        //the selector, the method, and how many of its last arguments may be left out
        c.add_sel(c"set", Self::set, 1)?;
        //what `#[sel("read", gimme)] #[defer_low]` gives: run later on the main thread
        c.add_sel_gimme_deferred(c"read", Self::read, Defer::Low)?;
        c.add_attr(c"foo", Self::foo, Self::set_foo, AttrOptions::new())?;
        Ok(())
    }
}

impl MaxObjWrapped for SimpExplicit {
    //`Err` refuses to make the object: the message is posted to the Max console and the box
    //shows that it couldn't be created
    fn new(builder: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        //an optional argument: the initial value. `?` passes a bad one on as the error
        let value = match builder.creation_args().first() {
            Some(a) => initial_value(a)?,
            None => 0,
        };

        //inlets, left to right. Inlet 0 always exists, this gives it an assist text
        builder.inlet(Inlet::default().assist("first inlet"));
        //a float inlet that calls a closure
        builder.inlet(
            Inlet::float(|_s: &Self, v| {
                post!("got float {}", v);
            })
            .assist("my float inlet"),
        );
        //an int inlet that calls a method; without an assist text Max shows "int"
        builder.inlet(Inlet::int(Self::int));
        //a proxy inlet: messages go to the usual methods, `self.obj.proxy_inlet()` says 3
        builder.inlet(Inlet::proxy().assist("my proxy inlet"));
        Ok(Self {
            obj: builder.obj(),
            value: Int64::new(value),
            fvalue: Default::default(),
            _v: String::from("blah"),
            clock: builder.clock(Self::clocked),
            changed: builder.queue(Self::report),
            list_out: builder.outlet(Outlet::list().assist("my list out")),
        })
    }

    //`class_setup` is still there, called after the methods are registered, for anything else
}

impl SimpExplicit {
    pub fn bang(&self) {
        let i = self.obj.proxy_inlet();
        median::object_post!(&self.obj, "from rust {} inlet {}", self.value, i);
        self.clock.delay(10);
    }

    pub fn int(&self, v: t_atom_long) {
        let i = self.obj.proxy_inlet();
        self.value.set(v);
        let _ = self.obj.attr_touch_with_name(c"value");
        post!("from rust {} inlet {}", self.value, i);
        self.changed.set();
    }

    pub fn read(&self, _sel: &SymbolRef, atoms: &[Atom]) {
        median::object_post!(
            &self.obj,
            "read {} argument(s) on the main thread",
            atoms.len()
        );
    }

    pub fn report(&self) {
        median::object_post!(&self.obj, "value is now {}", self.value);
    }

    pub fn list(&self, atoms: &[Atom]) {
        post!("got list with length {}", atoms.len());
    }

    pub fn baz(&self, sel: &SymbolRef, atoms: &[Atom]) {
        post!("got any with sel {} and length {}", sel, atoms.len());
    }

    pub fn set(&self, name: SymbolRef) -> Result<(), String> {
        if name == SymbolRef::default() {
            return Err("set needs a name".into());
        }
        median::object_post!(&self.obj, "set {}", name);
        Ok(())
    }

    pub fn foo(&self) -> f64 {
        self.fvalue.get()
    }

    pub fn set_foo(&self, v: f64) {
        self.fvalue.set(v);
    }

    pub fn clocked(&self) {
        post("clocked".to_string());
        let _ = self
            .list_out
            .send(&[1i64.into(), 12f64.into(), SymbolRef::from(c"foo").into()]);
    }
}

//the initial value: an int, or a float (truncated)
fn initial_value(a: &Atom) -> Result<t_atom_long, String> {
    match a.get_value() {
        Some(AtomValue::Int(v)) => Ok(v),
        Some(AtomValue::Float(v)) => Ok(v as t_atom_long),
        Some(AtomValue::Symbol(s)) => Err(format!("the initial value must be a number, not {s}")),
        _ => Err("the initial value must be a number".into()),
    }
}

median::ext_main! { SimpExplicit }
