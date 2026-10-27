//! Creation arguments (`median::args`): a class whose `new` reads its positional and attribute
//! arguments and records what it saw, created through the `median-mock` kernel, which processes
//! the `@name value` tail after `new` as Max's `attr_args_process` does.

use median::args::{Arg, ArgError, ArgTypeError, Args, FromAtom};
use median::prelude::*;
use median_mock::{Atom as MockAtom, Kernel, Level};
use std::sync::{Mutex, Once};

/// What the last `new` saw, one line each.
static LOG: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn log(line: String) {
    LOG.lock().unwrap().push(line);
}

fn take_log() -> Vec<String> {
    std::mem::take(&mut *LOG.lock().unwrap())
}

#[derive(MaxObject)]
#[max(name = "args.probe")]
pub struct Probe {
    #[attr]
    value: Int64,
    name: Mutex<SymbolRef>,
}

impl MaxObjWrapped for Probe {
    fn new(b: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        let args = b.args();
        log(format!(
            "raw {} positional {}",
            args.raw().len(),
            args.positional().len()
        ));
        log(format!("text [{}]", args.text()));
        log(format!("positional text [{}]", args.positional_text()));
        for (name, values) in args.attrs() {
            log(format!("attr {name} [{}]", Args::new(values).text()));
        }
        let first = args.at(0).get::<t_atom_long>()?;
        log(format!("arg0 {first:?}"));
        log(format!("arg1 {}", args.at(1).unwrap_or(-1.5f64)?));
        let value = args.attr(median::sym!("value"));
        log(format!(
            "attr value {:?} present {} values {:?}",
            value.get::<t_atom_long>()?,
            value.is_present(),
            value.values().map(<[_]>::len)
        ));
        log(format!("attr value text {:?}", value.text()));
        // the attribute has not been applied yet
        Ok(Self {
            value: Int64::new(-1),
            name: Mutex::new(SymbolRef::default()),
        })
    }
}

#[median::methods]
impl Probe {
    #[attr]
    fn name(&self) -> SymbolRef {
        *self.name.lock().unwrap()
    }

    fn set_name(&self, v: SymbolRef) {
        *self.name.lock().unwrap() = v;
    }
}

median::ext_main! { Probe }

fn setup() -> Kernel {
    static ONCE: Once = Once::new();
    let k = median_mock::lock();
    // SAFETY: that is how Max calls it, on its main thread
    ONCE.call_once(|| unsafe { ext_main(std::ptr::null_mut()) });
    take_log();
    k
}

fn s(v: &str) -> MockAtom {
    MockAtom::Sym(v.into())
}

fn make(k: &Kernel, args: &[MockAtom]) -> *mut std::ffi::c_void {
    let obj = k.new_object("args.probe", args);
    assert!(!obj.is_null(), "{args:?}: {:?}", k.console_log());
    assert!(k.take_console_log().is_empty());
    obj
}

fn value(k: &Kernel, obj: *mut std::ffi::c_void) -> MockAtom {
    k.attr_get(obj, "value").unwrap().remove(0)
}

#[test]
fn no_arguments() {
    let k = setup();
    let obj = make(&k, &[]);
    assert_eq!(
        take_log(),
        [
            "raw 0 positional 0",
            "text []",
            "positional text []",
            "arg0 None",
            "arg1 -1.5",
            "attr value None present false values None",
            "attr value text None",
        ]
    );
    assert_eq!(value(&k, obj), MockAtom::Long(-1));
    assert!(k.free_object(obj));
}

#[test]
fn positional_arguments_only() {
    let k = setup();
    let obj = make(&k, &[1.into(), 2.5.into(), s("foo")]);
    assert_eq!(
        take_log(),
        [
            "raw 3 positional 3",
            "text [1 2.5 foo]",
            "positional text [1 2.5 foo]",
            "arg0 Some(1)",
            "arg1 2.5",
            "attr value None present false values None",
            "attr value text None",
        ]
    );
    assert!(k.free_object(obj));
}

#[test]
fn positional_and_attribute_arguments() {
    let k = setup();
    let obj = make(
        &k,
        &[1.into(), s("@value"), 7.into(), s("@name"), s("x"), s("y")],
    );
    assert_eq!(
        take_log(),
        [
            "raw 6 positional 1",
            "text [1 @value 7 @name x y]",
            "positional text [1]",
            "attr value [7]",
            "attr name [x y]",
            "arg0 Some(1)",
            "arg1 -1.5",
            "attr value Some(7) present true values Some(1)",
            "attr value text Some(\"7\")",
        ]
    );
    // `new` saw 7, and the attribute setter ran after it with the same atoms
    assert_eq!(value(&k, obj), MockAtom::Long(7));
    assert_eq!(k.attr_get(obj, "name").unwrap(), [s("x")]);
    assert!(k.free_object(obj));
}

#[test]
fn attribute_arguments_only() {
    let k = setup();
    let obj = make(&k, &[s("@value"), 7.into()]);
    assert_eq!(
        take_log(),
        [
            "raw 2 positional 0",
            "text [@value 7]",
            "positional text []",
            "attr value [7]",
            // an `@name` is not a positional argument
            "arg0 None",
            "arg1 -1.5",
            "attr value Some(7) present true values Some(1)",
            "attr value text Some(\"7\")",
        ]
    );
    assert_eq!(value(&k, obj), MockAtom::Long(7));
    assert!(k.free_object(obj));
}

#[test]
fn a_repeated_attribute_takes_the_last_and_an_empty_one_has_no_value() {
    let k = setup();
    let obj = make(&k, &[s("@value"), 1.into(), s("@value"), 2.into()]);
    let log = take_log();
    assert!(
        log.contains(&"attr value Some(2) present true values Some(1)".to_string()),
        "{log:?}"
    );
    assert_eq!(value(&k, obj), MockAtom::Long(2));
    assert!(k.free_object(obj));

    // `@value` with nothing after it: present, with no values, so no first value to read
    let obj = k.new_object("args.probe", &[s("@value")]);
    assert!(!obj.is_null());
    let log = take_log();
    assert!(log.contains(&"attr value []".to_string()), "{log:?}");
    assert!(
        log.contains(&"attr value None present true values Some(0)".to_string()),
        "{log:?}"
    );
    assert!(
        log.contains(&"attr value text Some(\"\")".to_string()),
        "{log:?}"
    );
    assert!(k.free_object(obj));
}

/// Make `args`, expect it refused with `msg` posted for the object, and nothing left.
fn refused(k: &Kernel, args: &[MockAtom], msg: &str) {
    let obj = k.new_object("args.probe", args);
    assert!(obj.is_null(), "{args:?}");
    let console = k.take_console_log();
    assert_eq!(console.len(), 1, "{console:?}");
    assert_eq!(console[0].level, Level::Error);
    assert_eq!(console[0].text, msg);
    assert_ne!(console[0].object, 0);
    assert_eq!(k.live_object_count(), 0);
    take_log();
}

#[test]
fn a_bad_argument_refuses_the_object_with_a_message() {
    let k = setup();
    refused(&k, &[s("foo")], "argument 1: expected a number, got `foo`");
    refused(
        &k,
        &[1.into(), s("bar")],
        "argument 2: expected a number, got `bar`",
    );
    // a float for an integer argument is truncated, as Max does for a typed `A_LONG` argument
    let obj = make(&k, &[2.5.into()]);
    let log = take_log();
    assert!(log.contains(&"arg0 Some(2)".to_string()), "{log:?}");
    assert!(k.free_object(obj));
    refused(
        &k,
        &[s("@value"), s("x")],
        "@value: expected a number, got `x`",
    );
}

#[test]
fn text_of_each_kind_of_atom() {
    let _k = setup();
    assert_eq!(Atom::from(5i64).text(), "5");
    assert_eq!(Atom::from(-12i64).text(), "-12");
    assert_eq!(Atom::from(3.0).text(), "3.");
    assert_eq!(Atom::from(2.5).text(), "2.5");
    assert_eq!(Atom::from(median::sym!("foo")).text(), "foo");
    assert_eq!(Atom::from(median::sym!("@value")).text(), "@value");
    // an object atom is never inspected: safe code can put any pointer in one
    let bogus = Atom::from(0x10usize as *mut std::ffi::c_void);
    assert_eq!(bogus.text(), "<object>");
    assert_eq!(Args::new(std::slice::from_ref(&bogus)).text(), "<object>");
}

#[test]
fn a_list_longer_than_the_sdk_count_is_all_positional_without_an_attribute() {
    let _k = setup();
    let many: Vec<Atom> = (0..40_000i64).map(Atom::from).collect();
    let args = Args::new(&many);
    assert_eq!(args.positional().len(), 40_000);
    assert_eq!(args.attrs().count(), 0);
    assert!(args.at(39_999).is_present());
    // an `@name` inside the searched prefix still splits
    let mut with_attr: Vec<Atom> = (0..100i64).map(Atom::from).collect();
    with_attr.push(Atom::from(median::sym!("@x")));
    with_attr.extend((0..40_000i64).map(Atom::from));
    let args = Args::new(&with_attr);
    assert_eq!(args.positional().len(), 100);
    assert_eq!(
        args.attr(median::sym!("x")).values().map(|v| v.len()),
        Some(40_000)
    );
}

fn atoms(a: &[MockAtom]) -> Vec<Atom> {
    a.iter()
        .map(|a| match a {
            MockAtom::Long(v) => Atom::from(*v),
            MockAtom::Float(v) => Atom::from(*v),
            MockAtom::Sym(v) => Atom::from(SymbolRef::try_from(v.as_str()).unwrap()),
            other => panic!("{other:?}"),
        })
        .collect()
}

#[test]
fn from_atom_accepts_what_it_documents() {
    let _k = setup();
    let int = Atom::from(3i64);
    let float = Atom::from(2.5);
    let sym = Atom::from(median::sym!("foo"));
    let one = Atom::from(1i64);

    assert_eq!(t_atom_long::from_atom(&int), Ok(3));
    // truncated toward zero, like a typed `A_LONG` argument
    assert_eq!(t_atom_long::from_atom(&float), Ok(2));
    assert_eq!(t_atom_long::from_atom(&Atom::from(-2.5)), Ok(-2));
    assert!(t_atom_long::from_atom(&sym).is_err());

    assert_eq!(f64::from_atom(&int), Ok(3.0));
    assert_eq!(f64::from_atom(&float), Ok(2.5));
    assert!(f64::from_atom(&sym).is_err());

    assert_eq!(SymbolRef::from_atom(&sym), Ok(median::sym!("foo")));
    assert!(SymbolRef::from_atom(&int).is_err());

    assert_eq!(String::from_atom(&sym).as_deref(), Ok("foo"));
    assert_eq!(String::from_atom(&int).as_deref(), Ok("3"));
    assert_eq!(String::from_atom(&float).as_deref(), Ok("2.5"));

    assert_eq!(bool::from_atom(&Atom::from(0i64)), Ok(false));
    assert_eq!(bool::from_atom(&one), Ok(true));
    assert!(bool::from_atom(&Atom::from(2i64)).is_err());
    assert!(bool::from_atom(&Atom::from(1.0)).is_err());
    assert!(bool::from_atom(&sym).is_err());

    // an object atom is never accepted
    let mut x = 0u8;
    // SAFETY: the pointee is never inspected, the atom is only given to the conversions
    let obj = unsafe { Atom::from_object_raw((&raw mut x).cast()) };
    assert!(t_atom_long::from_atom(&obj).is_err());
    assert!(f64::from_atom(&obj).is_err());
    assert!(SymbolRef::from_atom(&obj).is_err());
    assert!(String::from_atom(&obj).is_err());
    assert!(bool::from_atom(&obj).is_err());
    assert_eq!(f64::from_atom(&obj).unwrap_err().found, "an object");
}

#[test]
fn args_split_a_list_without_a_builder() {
    let _k = setup();
    let list = atoms(&[s("a"), 1.into(), s("@x"), 2.into(), 3.into(), s("@y")]);
    let args = Args::new(&list);
    assert_eq!(args.raw().len(), 6);
    assert_eq!(args.positional().len(), 2);
    let attrs: Vec<_> = args
        .attrs()
        .map(|(n, v)| (n.to_string(), v.len()))
        .collect();
    assert_eq!(attrs, [("x".to_string(), 2), ("y".to_string(), 0)]);
    assert_eq!(
        args.attr(median::sym!("x")).values().map(<[_]>::len),
        Some(2)
    );
    assert_eq!(
        args.attr(median::sym!("y")).values().map(<[_]>::len),
        Some(0)
    );
    assert!(args.attr(median::sym!("y")).is_present());
    assert!(args.attr(median::sym!("y")).atom().is_none());
    assert!(!args.attr(median::sym!("z")).is_present());
    assert!(args.attr(median::sym!("z")).values().is_none());
    assert_eq!(args.attr(median::sym!("x")).text().as_deref(), Some("2 3"));
    assert_eq!(args.attr(median::sym!("y")).text().as_deref(), Some(""));
    assert_eq!(args.attr(median::sym!("z")).text(), None);
    assert_eq!(
        args.attr(median::sym!("x")).get::<t_atom_long>(),
        Ok(Some(2))
    );
    assert_eq!(args.attr(median::sym!("y")).get::<t_atom_long>(), Ok(None));
    assert_eq!(args.attr(median::sym!("z")).unwrap_or(9), Ok(9));
    assert_eq!(
        args.attr(median::sym!("y"))
            .unwrap_or_default::<t_atom_long>(),
        Ok(0)
    );
    assert_eq!(
        t_atom_long::try_from(args.attr(median::sym!("z"))),
        Err(ArgError::MissingAttr {
            name: median::sym!("z")
        })
    );
    assert_eq!(
        t_atom_long::try_from(args.attr(median::sym!("y")))
            .unwrap_err()
            .to_string(),
        "@y: missing"
    );
    assert_eq!(
        SymbolRef::try_from(args.attr(median::sym!("x")))
            .unwrap_err()
            .to_string(),
        "@x: expected a symbol, got `2`"
    );
    // positional
    assert!(args.at(0).is_present());
    assert!(!args.at(2).is_present());
    assert_eq!(args.at(0).values().map(<[_]>::len), Some(1));
    assert_eq!(args.at(0).atom().map(Atom::text).as_deref(), Some("a"));
    assert_eq!(args.at(1).text().as_deref(), Some("1"));
    assert_eq!(args.at(2).text(), None);
    assert_eq!(SymbolRef::try_from(args.at(0)), Ok(median::sym!("a")));
    assert_eq!(
        t_atom_long::try_from(args.at(5)),
        Err(ArgError::Missing { index: 5 })
    );
    assert_eq!(args.at(5).get::<t_atom_long>(), Ok(None));
    assert_eq!(args.at(7).unwrap_or::<t_atom_long>(9), Ok(9));
    assert_eq!(args.at(7).unwrap_or_default::<String>(), Ok(String::new()));
    assert_eq!(args.at(1).unwrap_or_default::<t_atom_long>(), Ok(1));
    let e = args.at(0).get::<t_atom_long>().unwrap_err();
    assert_eq!(e.to_string(), "argument 1: expected a number, got `a`");
    assert_eq!(args.text(), "a 1 @x 2 3 @y");
    assert_eq!(
        NewError::from(e).message(),
        "argument 1: expected a number, got `a`"
    );
}

/// A user type that wants to see where its argument came from: an enum from a symbol.
#[derive(Debug, PartialEq, Eq)]
enum Mode {
    Fast,
    Slow,
}

impl TryFrom<Arg<'_>> for Mode {
    type Error = ArgError;

    fn try_from(arg: Arg<'_>) -> Result<Self, ArgError> {
        let Some(a) = arg.atom() else {
            return Err(arg.missing());
        };
        match a.text().as_str() {
            "fast" => Ok(Mode::Fast),
            "slow" => Ok(Mode::Slow),
            _ => Err(arg.type_error(ArgTypeError::new("fast or slow", a))),
        }
    }
}

#[test]
fn a_user_type_reads_an_arg_through_try_from() {
    let _k = setup();
    let list = atoms(&[s("slow"), s("loud"), s("@mode"), s("fast"), s("@empty")]);
    let args = Args::new(&list);

    let m: Mode = args.at(0).try_into().unwrap();
    assert_eq!(m, Mode::Slow);
    assert_eq!(args.at(0).unwrap_or(Mode::Fast), Ok(Mode::Slow));
    assert_eq!(
        args.at(0).unwrap_or_default::<String>().as_deref(),
        Ok("slow")
    );
    assert_eq!(args.at(0).get::<Mode>(), Ok(Some(Mode::Slow)));
    assert_eq!(
        args.attr(median::sym!("mode")).get::<Mode>(),
        Ok(Some(Mode::Fast))
    );

    // absent: the default, `None`, an error that names the argument
    assert_eq!(args.at(4).unwrap_or(Mode::Fast), Ok(Mode::Fast));
    assert_eq!(args.at(4).get::<Mode>(), Ok(None));
    assert_eq!(
        Mode::try_from(args.at(4)),
        Err(ArgError::Missing { index: 4 })
    );
    assert_eq!(args.attr(median::sym!("empty")).get::<Mode>(), Ok(None));
    assert_eq!(
        Mode::try_from(args.attr(median::sym!("empty"))),
        Err(ArgError::MissingAttr {
            name: median::sym!("empty")
        })
    );

    // present but wrong: never the default, the error says where
    let e = args.at(1).unwrap_or(Mode::Fast).unwrap_err();
    assert_eq!(
        e.to_string(),
        "argument 2: expected fast or slow, got `loud`"
    );
    assert_eq!(
        args.at(1).get::<Mode>().unwrap_err().to_string(),
        "argument 2: expected fast or slow, got `loud`"
    );
    let r: Result<Mode, _> = args.at(1).try_into();
    assert_eq!(
        r.unwrap_err().to_string(),
        "argument 2: expected fast or slow, got `loud`"
    );
}
