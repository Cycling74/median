use median::{atom::AtomValue, gimmeback::Reply, prelude::*};

//the struct is an ordinary struct: `#[derive(MaxObj)]` makes it a Max class named "simp"
//(the type name in lower case, or `#[max(name = "..")]`), with a `Simp::register()` that
//`ext_main!` below calls.
#[derive(MaxObj)]
//`description`, `author` and `tags` are maxref metadata (`tags` is split on commas); nothing reads
//them yet.
#[max(name = "simp", description = "A simple example object")]
#[max(author = "median", tags = "example, median")]
pub struct Simp {
    obj: ObjRef,
    //an attribute "value" backed by this field, clipped to 0..=100. The field's type must be
    //one of the `median::num` atomics (anything that implements `median::attr::AttrField`):
    //Max reads and writes attributes from other threads while methods run. In the inspector it
    //is "Value" on the "Simp" tab, and its value is saved with the patcher.
    #[attr(clip(0, 100), label = "Value", category = "Simp", save)]
    value: Int64,
    fvalue: Float64,
    _v: String,
    clock: Clock,
    //reports the value on the main thread, once however often it changes in between
    changed: Queue,
    list_out: Outlet<List>,
}

//implement the max object wrapper
impl MaxObjWrapped for Simp {
    //create an instance of your object
    //setup inlets/outlets and clocks
    //`Err` refuses to make the object: the message is posted to the Max console and the box
    //shows that it couldn't be created
    fn new(builder: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        //an optional first argument: the initial value, an int or a float (truncated), 0 if left
        //out. A bad one is an `ArgError`, which `?` passes on as the error. `@value 7` is not an
        //argument here: `builder.args()` keeps the attribute arguments apart, and the attribute
        //is set once `new` has returned
        let value = builder.args().at(0).unwrap_or(0)?;

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
        self.changed.set();
    }

    //an "int" method for the leftmost and proxy inlets; the typed inlet uses `int` above
    #[int]
    pub fn proxied(&self, v: t_atom_long) {
        let i = self.obj.proxy_inlet();
        self.value.set(v);
        let _ = self.obj.attr_touch_with_name(c"value");
        post!("proxied from rust {} inlet {}", self.value, i);
        self.changed.set();
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

    //"read" with any arguments, run later on the main thread whichever thread sent it (as a
    //C external would `defer_low` it, for example to open a file dialog). The arguments are
    //copied for the deferred call.
    #[gimme("read")]
    #[defer_low]
    pub fn read(&self, atoms: &[Atom]) {
        median::object_post!(
            &self.obj,
            "read {} argument(s) on the main thread",
            atoms.len()
        );
    }

    //"doAbs" is the SDK's `simplejs` example: the absolute value of the number(s) it is given,
    //for a typed caller that reads the reply, such as another external calling
    //`object_method_typed` (`doAbs -3` is 3, `doAbs 1 -2` is the list [1, 2]). The method
    //returns what the caller gets back: a `Reply`, a plain value that converts to one, or a
    //`Result` of one (an error is posted to the console and the caller gets no value). A patch
    //cord cannot reach it, so it is marked undocumented and not listed in the patcher; `js`'s
    //`Maxobj.message` runs it but never hands the reply back, and would leak a list reply
    //(observed 2026-10-27), so a method meant for `js` should reply with a scalar.
    #[gimmeback("doAbs")]
    pub fn do_abs(&self, atoms: &[Atom]) -> Result<Reply, String> {
        match atoms {
            [] => Err("missing argument for method doAbs()".into()),
            [one] => Ok(match one.get_value() {
                Some(AtomValue::Int(i)) => Reply::Int(checked_abs(i)?),
                Some(AtomValue::Float(f)) => Reply::Float(f.abs()),
                _ => Reply::Nothing,
            }),
            many => Ok(Reply::Atoms(
                many.iter()
                    .map(|a| {
                        Ok(match a.get_value() {
                            Some(AtomValue::Int(i)) => Atom::from(checked_abs(i)?),
                            Some(AtomValue::Float(f)) => Atom::from(f.abs()),
                            //a symbol goes through as it is (an `Atom` is not `Clone`)
                            _ => Atom::from(a.get_symbol()),
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?,
            )),
        }
    }

    //Max calls this for a notification from an object this one is attached to. The notification
    //is valid for the call only. A method can't be deferred (`#[defer]`) for that reason.
    #[notify]
    pub fn notified(&self, n: &Notification) {
        median::object_post!(&self.obj, "notification {}", n.message());
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

    //called on the main thread via the `changed` queue (see the `int` method)
    pub fn report(&self) {
        median::object_post!(&self.obj, "value is now {}", self.value);
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

//the `ext_main` that Max calls to register the class
median::ext_main! { Simp }

//`t_atom_long::MIN` has no absolute value of its own type: an error, not a wrapped result
fn checked_abs(i: median::max_sys::t_atom_long) -> Result<median::max_sys::t_atom_long, String> {
    i.checked_abs()
        .ok_or_else(|| format!("doAbs: {i} is out of range"))
}
