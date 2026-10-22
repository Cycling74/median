//! Inlet and outlet descriptors: positions and creation order (checked through the
//! `median-mock` kernel's inlet records), `int`/`float` inlet dispatch, proxy ids, assist text from
//! the table, the hook and the defaults, truncation, `inletinfo`, the `undocumented` flag on the
//! wrapper's own methods, typed outlets, and descriptions that are mistakes.
//!
//! Every test takes `median_mock::lock()` for its whole body; the classes are registered once.

use median::prelude::*;
use median::wrapper::ASSIST_MAX_STRING_LEN;
use median_mock::{Atom as MockAtom, InletKind, Kernel, Level};
use std::borrow::Cow;
use std::sync::Once;

/// A Max object with every kind of inlet and outlet.
#[derive(MaxObject)]
#[max(name = "io.max")]
pub struct IoMax {
    obj: ObjRef,
    bang: Outlet<Bang>,
    int: Outlet<Int>,
    float: Outlet<Float>,
    list: Outlet<List>,
    any: Outlet<Any>,
}

impl MaxObjWrapped for IoMax {
    fn new(b: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        let ids = [
            b.inlet(Inlet::default().assist("left")),
            b.inlet(Inlet::float(|s: &Self, v| s.obj.post(format!("ft {v}"))).assist("f")),
            b.inlet(Inlet::int(|s: &Self, v| {
                s.obj.post(format!("first int {v}"))
            })),
            b.inlet(Inlet::proxy().hot()),
            b.inlet(Inlet::int(|s: &Self, v| s.obj.post(format!("second int {v}"))).assist("i2")),
            b.inlet(Inlet::proxy().assist("p5")),
        ];
        let positions: Vec<usize> = ids.iter().map(|i| i.index()).collect();
        assert_eq!(positions, [0, 1, 2, 3, 4, 5]);
        Ok(Self {
            obj: b.obj(),
            bang: b.outlet(Outlet::bang()),
            int: b.outlet(Outlet::int().assist("an int")),
            float: b.outlet(Outlet::float()),
            list: b.outlet(Outlet::list().assist("my list")),
            any: b.outlet(Outlet::any()),
        })
    }
}

#[median::methods]
impl IoMax {
    #[bang]
    pub fn bang(&self) {
        self.obj
            .post(format!("bang from {}", self.obj.proxy_inlet()));
    }

    #[sel("fire")]
    pub fn fire(&self) {
        let results = [
            self.bang.send(),
            self.int.send(3),
            self.float.send(1.5),
            self.list.send(&[1i64.into(), 2.5.into()]),
            self.any.send(&SymbolRef::from(c"foo"), &[7i64.into()]),
            self.any.send_bang(),
            self.any.send_int(4),
            self.any.send_float(0.25),
            self.any.send_list(&[SymbolRef::from(c"x").into()]),
        ];
        let ok = results.iter().filter(|r| r.is_ok()).count();
        let overflow = results.iter().filter(|r| **r == Err(StackOverflow)).count();
        self.obj.post(format!("sent {ok}, overflow {overflow}"));
    }
}

/// Assist and hot/cold from the hooks, which depend on state; inlet 2's table text is too long.
#[derive(MaxObject)]
#[max(name = "io.hooked")]
pub struct Hooked {
    open: Int64,
}

/// 200 three byte characters: 600 bytes.
fn euros() -> String {
    "€".repeat(200)
}

impl MaxObjWrapped for Hooked {
    fn new(b: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        b.inlet(Inlet::proxy().assist("from the table"));
        b.inlet(Inlet::proxy().assist(&euros()));
        b.inlet(Inlet::proxy().hot());
        Ok(Self {
            open: Int64::new(0),
        })
    }

    fn assist(&self, iolet: IoLet) -> Option<Cow<'_, str>> {
        match iolet {
            IoLet::Inlet(1) if self.open.get() != 0 => Some("currently open".into()),
            IoLet::Inlet(1) => None,
            //longer than Max takes, two byte characters
            IoLet::Outlet(0) => Some("é".repeat(300).into()),
            //a one byte character before the cut, then three byte ones across it
            IoLet::Outlet(1) => Some(format!("a{}", euros()).into()),
            _ => None,
        }
    }

    fn inlet_hot(&self, index: usize) -> Option<bool> {
        (index == 1).then(|| self.open.get() != 0)
    }
}

#[median::methods]
impl Hooked {
    #[int]
    pub fn int(&self, v: t_atom_long) {
        self.open.set(v);
    }
}

/// An MSP object: three signal inlets, then a proxy and a float inlet.
#[derive(MaxObject)]
#[max(name = "io.dsp~", kind = msp)]
pub struct IoDsp {
    obj: ObjRef,
}

impl MSPObjWrapped for IoDsp {
    fn new(b: &mut MSPBuilder<Self>) -> Result<Self, NewError> {
        let ids = [
            b.inlet(Inlet::signal().assist("sig l")),
            b.inlet(Inlet::signal()),
            b.inlet(Inlet::signal().hot()),
            b.inlet(Inlet::proxy().assist("p")),
            b.inlet(Inlet::float(|s: &Self, v| s.obj.post(format!("ft {v}")))),
        ];
        assert_eq!(ids.map(|i| i.index()), [0, 1, 2, 3, 4]);
        b.outlet(Outlet::signal().assist("out"));
        b.outlet(Outlet::list());
        Ok(Self { obj: b.obj() })
    }

    fn perform(&self, ins: &[&[f64]], outs: &mut [&mut [f64]], _nframes: usize) {
        for (i, o) in outs[0].iter_mut().enumerate() {
            *o = ins[0][i] + ins[1][i] + ins[2][i];
        }
    }
}

#[median::methods]
impl IoDsp {
    #[bang]
    pub fn bang(&self) {
        self.obj
            .post(format!("bang from {}", self.obj.proxy_inlet()));
    }
}

/// An MSP object with no signal inlets: inlet 0 is still there, with its own assist.
#[derive(MaxObject)]
#[max(name = "io.nosig~", kind = msp, no_methods)]
pub struct NoSig;

impl MSPObjWrapped for NoSig {
    fn new(b: &mut MSPBuilder<Self>) -> Result<Self, NewError> {
        b.inlet(Inlet::default().assist("control"));
        b.inlet(Inlet::proxy());
        b.outlet(Outlet::signal());
        Ok(NoSig)
    }

    fn perform(&self, _ins: &[&[f64]], _outs: &mut [&mut [f64]], _nframes: usize) {}
}

/// Mistakes in the description, picked by the first argument.
#[derive(MaxObject)]
#[max(name = "io.bad~", kind = msp, no_methods)]
pub struct Bad {
    _clock: Clock,
}

impl MSPObjWrapped for Bad {
    fn new(b: &mut MSPBuilder<Self>) -> Result<Self, NewError> {
        let which = b.creation_args().first().map_or(0, |a| a.get_int());
        b.outlet(Outlet::list());
        match which {
            //a signal inlet after a proxy
            0 => {
                b.inlet(Inlet::proxy());
                b.inlet(Inlet::signal());
            }
            //`Inlet::default()` that isn't first
            1 => {
                b.inlet(Inlet::proxy());
                b.inlet(Inlet::default());
            }
            //ten int inlets
            _ => {
                for _ in 0..10 {
                    b.inlet(Inlet::int(|_: &Self, _| {}));
                }
            }
        }
        Ok(Self {
            _clock: b.clock(|_| {}),
        })
    }

    fn perform(&self, _ins: &[&[f64]], _outs: &mut [&mut [f64]], _nframes: usize) {}
}

median::ext_main! { IoMax, Hooked, IoDsp, NoSig, Bad }

fn setup() -> Kernel {
    static ONCE: Once = Once::new();
    let k = median_mock::lock();
    // SAFETY: that is how Max calls it, on its main thread
    ONCE.call_once(|| unsafe { ext_main(std::ptr::null_mut()) });
    k
}

fn posts(k: &Kernel) -> Vec<String> {
    k.take_console_log()
        .into_iter()
        .map(|e| {
            assert_eq!(e.level, Level::Post, "{e:?}");
            e.text
        })
        .collect()
}

#[test]
fn max_inlets_are_made_right_to_left_at_their_positions() {
    let k = setup();
    let obj = k.new_object("io.max", &[]);
    assert!(!obj.is_null());
    //positions 1..: `in`/`ft` numbers count each kind from 1, proxies get their position
    assert_eq!(
        k.inlets(obj),
        [
            InletKind::Float(1),
            InletKind::Int(1),
            InletKind::Proxy { id: 3, stuffloc: 0 },
            InletKind::Int(2),
            InletKind::Proxy { id: 5, stuffloc: 0 },
        ]
    );
    let mut created = k.inlets(obj);
    created.reverse();
    assert_eq!(k.inlets_created(obj), created);
    assert!(k.free_object(obj));
    assert!(k.console_log().is_empty(), "{:?}", k.console_log());
}

#[test]
fn int_and_float_inlets_call_their_callbacks_and_proxies_report_their_index() {
    let k = setup();
    let obj = k.new_object("io.max", &[]);
    k.send_inlet(obj, 1, "float", &[MockAtom::Float(1.5)])
        .unwrap();
    k.send_inlet(obj, 2, "int", &[MockAtom::Long(7)]).unwrap();
    k.send_inlet(obj, 4, "int", &[MockAtom::Long(8)]).unwrap();
    k.send_bang(obj).unwrap();
    k.send_inlet(obj, 3, "bang", &[]).unwrap();
    k.send_inlet(obj, 5, "bang", &[]).unwrap();
    assert_eq!(
        posts(&k),
        [
            "ft 1.5",
            "first int 7",
            "second int 8",
            "bang from 0",
            "bang from 3",
            "bang from 5"
        ]
    );
    assert!(k.free_object(obj));
}

#[test]
fn assist_comes_from_the_table_or_the_kind() {
    let k = setup();
    let obj = k.new_object("io.max", &[]);
    let inlets: Vec<_> = (0..7).map(|i| k.assist(obj, false, i).unwrap()).collect();
    assert_eq!(
        inlets,
        ["left", "f", "int", "inlet 4", "i2", "p5", "inlet 7"]
    );
    let outlets: Vec<_> = (0..6).map(|i| k.assist(obj, true, i).unwrap()).collect();
    assert_eq!(
        outlets,
        ["bang", "an int", "float", "my list", "anything", "outlet 6"]
    );
    assert!(k.free_object(obj));
}

#[test]
fn inletinfo_is_hot_for_inlet_0_and_marked_inlets() {
    let k = setup();
    let obj = k.new_object("io.max", &[]);
    let hot: Vec<_> = (0..6).map(|i| k.inlet_is_hot(obj, i).unwrap()).collect();
    assert_eq!(hot, [true, false, false, true, false, false]);
    assert!(k.free_object(obj));
}

#[test]
fn the_hooks_come_first_and_assist_is_cut_at_500_bytes_on_a_character() {
    let k = setup();
    let obj = k.new_object("io.hooked", &[]);
    //the table, then the hook overriding it once the state says so
    assert_eq!(k.assist(obj, false, 1).unwrap(), "from the table");
    assert_eq!(k.inlet_is_hot(obj, 1), Some(false));
    k.send_int(obj, 1).unwrap();
    assert_eq!(k.assist(obj, false, 1).unwrap(), "currently open");
    assert_eq!(k.inlet_is_hot(obj, 1), Some(true));
    //the table's `.hot()` where the hook says nothing
    assert_eq!(k.inlet_is_hot(obj, 3), Some(true));
    assert_eq!(k.inlet_is_hot(obj, 2), Some(false));
    assert_eq!(k.inlet_is_hot(obj, 0), Some(true));
    //no text and no hook: the leftmost
    assert_eq!(k.assist(obj, false, 0).unwrap(), "inlet 1");

    //too long, from the table: 166 whole euros (498 bytes)
    let s = k.assist(obj, false, 2).unwrap();
    assert_eq!(s, "€".repeat(166));
    //from the hook: 250 two byte characters fill exactly 500
    let s = k.assist(obj, true, 0).unwrap();
    assert_eq!(s, "é".repeat(250));
    assert_eq!(s.len(), ASSIST_MAX_STRING_LEN);
    //`a` and 166 euros (499 bytes)
    let s = k.assist(obj, true, 1).unwrap();
    assert_eq!(s, format!("a{}", "€".repeat(166)));
    assert!(k.console_log().is_empty(), "{:?}", k.console_log());
    assert!(k.free_object(obj));
}

#[test]
fn the_wrappers_own_methods_are_undocumented() {
    let k = setup();
    let obj = k.new_object("io.max", &[]);
    for i in 1..=9 {
        for sel in [format!("in{i}"), format!("ft{i}")] {
            assert_eq!(
                k.method_attr(obj, &sel, "undocumented"),
                Some(("long".to_string(), "1".to_string())),
                "{sel}"
            );
        }
    }
    //the user's methods are not
    assert_eq!(k.method_attr(obj, "bang", "undocumented"), None);
    //`inletinfo` is always there
    assert!(k.method(obj, "inletinfo").is_some());
    assert!(k.free_object(obj));
}

#[test]
fn typed_outlets_send_with_the_right_outlet_calls() {
    let k = setup();
    let obj = k.new_object("io.max", &[]);
    assert_eq!(
        k.outlet_types(obj),
        [
            Some("bang".to_string()),
            Some("int".to_string()),
            Some("float".to_string()),
            Some("list".to_string()),
            None
        ]
    );
    k.send(obj, "fire", &[]).unwrap();
    let sent: Vec<_> = k
        .take_outlet_log()
        .into_iter()
        .map(|e| (e.index, e.selector, e.atoms))
        .collect();
    assert_eq!(
        sent,
        [
            (0, "bang".to_string(), vec![]),
            (1, "int".to_string(), vec![MockAtom::Long(3)]),
            (2, "float".to_string(), vec![MockAtom::Float(1.5)]),
            (
                3,
                "list".to_string(),
                vec![MockAtom::Long(1), MockAtom::Float(2.5)]
            ),
            (4, "foo".to_string(), vec![MockAtom::Long(7)]),
            (4, "bang".to_string(), vec![]),
            (4, "int".to_string(), vec![MockAtom::Long(4)]),
            (4, "float".to_string(), vec![MockAtom::Float(0.25)]),
            (4, "list".to_string(), vec![MockAtom::Sym("x".into())]),
        ]
    );
    //every send succeeded: Max's outlet calls return non-null unless the stack overflowed
    let log = k.take_console_log();
    assert_eq!(
        log.last().map(|e| e.text.as_str()),
        Some("sent 9, overflow 0")
    );
    //a stack overflow: each send is an `Err(StackOverflow)` and nothing goes out
    k.set_outlet_overflow(true);
    k.send(obj, "fire", &[]).unwrap();
    assert!(k.take_outlet_log().is_empty());
    let log = k.take_console_log();
    assert_eq!(
        log.last().map(|e| e.text.as_str()),
        Some("sent 0, overflow 9")
    );
    k.set_outlet_overflow(false);
    assert!(k.free_object(obj));
}

#[test]
fn msp_signal_inlets_come_first_and_dsp_setup_runs_after_the_others() {
    let k = setup();
    let obj = k.new_object("io.dsp~", &[]);
    assert!(!obj.is_null());
    assert_eq!(k.signal_inlets(obj), Some(3));
    //the other inlets right to left, then `z_dsp_setup` (the two signal inlets after the
    //leftmost). The mock inserts every inlet, `z_dsp_setup`'s too, just right of the leftmost,
    //so this order is what puts the signal inlets at 0..3
    assert_eq!(
        k.inlets_created(obj),
        [
            InletKind::Float(1),
            InletKind::Proxy { id: 3, stuffloc: 0 },
            InletKind::Signal,
            InletKind::Signal,
        ]
    );
    assert_eq!(
        k.inlets(obj),
        [
            InletKind::Signal,
            InletKind::Signal,
            InletKind::Proxy { id: 3, stuffloc: 0 },
            InletKind::Float(1),
        ]
    );
    let inlets: Vec<_> = (0..5).map(|i| k.assist(obj, false, i).unwrap()).collect();
    assert_eq!(inlets, ["sig l", "signal", "signal", "p", "float"]);
    let outlets: Vec<_> = (0..2).map(|i| k.assist(obj, true, i).unwrap()).collect();
    assert_eq!(outlets, ["out", "list"]);
    let hot: Vec<_> = (0..5).map(|i| k.inlet_is_hot(obj, i).unwrap()).collect();
    assert_eq!(hot, [true, false, true, false, false]);

    k.send_inlet(obj, 3, "bang", &[]).unwrap();
    k.send_inlet(obj, 4, "float", &[MockAtom::Float(0.5)])
        .unwrap();
    assert_eq!(posts(&k), ["bang from 3", "ft 0.5"]);

    let adds = k.dsp64(obj, 48000., 2).unwrap();
    let mut ins = vec![vec![1., 2.], vec![10., 20.], vec![100., 200.]];
    let mut outs = vec![vec![0.; 2]];
    adds[0].run(&mut ins, &mut outs);
    assert_eq!(outs[0], [111., 222.]);

    assert!(k.free_object(obj));
    assert_eq!(k.dsp_frees(), [obj as usize]);
    assert!(k.console_log().is_empty(), "{:?}", k.console_log());
}

#[test]
fn msp_object_without_signal_inlets_keeps_inlet_0() {
    let k = setup();
    let obj = k.new_object("io.nosig~", &[]);
    assert_eq!(k.signal_inlets(obj), Some(0));
    assert_eq!(k.inlets(obj), [InletKind::Proxy { id: 1, stuffloc: 0 }]);
    assert_eq!(k.assist(obj, false, 0).unwrap(), "control");
    assert_eq!(k.assist(obj, false, 1).unwrap(), "inlet 2");
    assert_eq!(k.assist(obj, true, 0).unwrap(), "signal");
    assert!(k.free_object(obj));
    assert!(k.console_log().is_empty(), "{:?}", k.console_log());
}

#[test]
fn a_mistake_in_the_description_fails_the_object_and_frees_it() {
    let k = setup();
    let clocks = k.live_clock_count();
    for (arg, msg) in [
        (0, "signal inlets must come before the other inlets"),
        (
            1,
            "Inlet::default() describes inlet 0 and must be the first inlet",
        ),
        (2, "at most 9 int and 9 float inlets are supported"),
    ] {
        let obj = k.new_object("io.bad~", &[MockAtom::Long(arg)]);
        assert!(obj.is_null());
        let log = k.take_console_log();
        assert_eq!(log.len(), 1, "{log:?}");
        assert_eq!(log[0].level, Level::Error);
        assert!(log[0].text.contains(msg), "{log:?}");
        assert_eq!(k.live_object_count(), 0);
        assert_eq!(k.live_clock_count(), clocks);
        //nothing was set up for dsp, so nothing is freed either
        assert!(k.dsp_frees().is_empty());
    }
}
