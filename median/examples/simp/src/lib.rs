use median::{atom::AtomValue, prelude::*};

//the struct is an ordinary struct: `#[derive(MaxObject)]` makes it a Max class named "simp"
//(the type name in lower case, or `#[max(name = "..")]`), with a `Simp::register()` that
//`ext_main!` below calls.
#[derive(MaxObject)]
#[max(name = "simp")]
pub struct Simp {
    obj: ObjRef,
    //an attribute "value" backed by this field, clipped to 0..=100. The field's type must be
    //one of the `median::num` atomics (anything that implements `median::attr::AttrField`):
    //Max reads and writes attributes from other threads while methods run.
    #[attr(clip(0, 100))]
    value: Int64,
    fvalue: Float64,
    _v: String,
    clock: ClockHandle,
    list_out: Outlet<List>,
}

//implement the max object wrapper
impl MaxObjWrapped for Simp {
    //create an instance of your object
    //setup inlets/outlets and clocks
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
            clock: builder.with_clockfn(Self::clocked),
            list_out: builder.outlet(Outlet::list().assist("my list out")),
        })
    }
}

//the methods are ordinary methods; `#[median::methods]` registers the marked ones with Max (the
//impl is emitted as written, without the markers)
#[median::methods]
impl Simp {
    //a "bang" method. The name of the method can be anything you want
    #[bang]
    pub fn bang(&self) {
        let i = self.obj.proxy_inlet();
        median::object_post!(&self.obj, "from rust {} inlet {}", self.value, i);
        self.clock.delay(10);
    }

    //the typed integer inlet calls this method directly
    pub fn int(&self, v: t_atom_long) {
        let i = self.obj.proxy_inlet();
        self.value.set(v);
        let _ = self.obj.attr_touch_with_name(c"value");
        post!("direct int from rust {} inlet {}", self.value, i);
    }

    //an "int" method: takes 1 argument other than &self, of type t_atom_long
    #[int]
    pub fn proxied(&self, v: t_atom_long) {
        let i = self.obj.proxy_inlet();
        self.value.set(v);
        let _ = self.obj.attr_touch_with_name(c"value");
        post!("proxied from rust {} inlet {}", self.value, i);
    }

    //a "list" method
    #[list]
    pub fn list(&self, atoms: &[Atom]) {
        post!("got list with length {}", atoms.len());
    }

    //an "anything" method: gets the selector too
    #[any]
    pub fn baz(&self, sel: &SymbolRef, atoms: &[Atom]) {
        post!("got any with sel {} and length {}", sel, atoms.len());
    }

    //a "set" message with a symbol argument, which may be left out (it defaults to the empty
    //symbol). A method can return `Result<(), E>` (any `E: Display`): the error is posted to the
    //console for the object.
    #[sel("set", defaults = 1)]
    pub fn set(&self, name: SymbolRef) -> Result<(), String> {
        if name == SymbolRef::default() {
            return Err("set needs a name".into());
        }
        median::object_post!(&self.obj, "set {}", name);
        Ok(())
    }

    //an attribute "foo": the getter is marked, `set_foo` is found by its name (without one the
    //attribute is read-only)
    #[attr]
    pub fn foo(&self) -> f64 {
        self.fvalue.get()
    }

    pub fn set_foo(&self, v: f64) {
        self.fvalue.set(v);
    }

    //called via the `clock` member (see the `bang` method and `new` above)
    pub fn clocked(&self) {
        post("clocked".to_string());
        let _ = self.list_out.send(&[
            self.value.get().into(),
            self.fvalue.get().into(),
            SymbolRef::from(c"foo").into(),
        ]);
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

//the `ext_main` that Max calls to register the class
median::ext_main! { Simp }
