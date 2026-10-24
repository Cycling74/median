use median::{
    jit::{
        DrawContext, JitBuilder, JitResult, MatrixOp, Ob3d,
        matrix::{Count, IOCount, Matrix},
    },
    prelude::*,
};
use median_mock::{Atom as MockAtom, InletKind};
use std::sync::atomic::{AtomicUsize, Ordering};

static DROPS: AtomicUsize = AtomicUsize::new(0);
#[derive(MaxObject)]
#[max(name = "jit.iolets.test", kind = matrix_op)]
struct Extra {
    obj: ObjRef,
    out: Outlet<Int>,
    bang: Outlet<Bang>,
}
impl MatrixOp for Extra {
    fn io_count() -> IOCount {
        IOCount {
            inputs: Count::Fixed(2),
            outputs: Count::Fixed(2),
        }
    }
    fn new(b: &mut JitBuilder<Self>) -> Result<Self, NewError> {
        let mode = b.creation_args().first().map_or(0, |a| a.get_int());
        let out = b.outlet(Outlet::int().assist("result"))?;
        let bang = b.outlet(Outlet::bang().assist("done"))?;
        assert_eq!(
            b.inlet(
                Inlet::int(|s: &Self, v| {
                    s.out.send(v).unwrap();
                })
                .assist("integer")
            )?
            .index(),
            2
        );
        assert_eq!(
            b.inlet(
                Inlet::float(|s: &Self, v| {
                    assert!(v >= 0., "inlet panic");
                    s.obj.post(format!("float {v}"));
                })
                .hot()
                .assist("float")
            )?
            .index(),
            3
        );
        assert_eq!(b.inlet(Inlet::proxy().assist("messages"))?.index(), 4);
        match mode {
            1 => return Err("refused with outlets".into()),
            2 => panic!("constructor with outlets"),
            3 => {
                b.inlet(Inlet::default())?;
            }
            4 => {
                for _ in 0..9 {
                    b.inlet(Inlet::int(|_: &Self, _| {}))?;
                }
            }
            _ => (),
        }
        Ok(Self {
            obj: b.max_obj()?,
            out,
            bang,
        })
    }
    fn calc(&self, _: &mut [Matrix<'_>], _: &mut [Matrix<'_>]) -> JitResult<()> {
        Ok(())
    }
}
#[median::methods]
impl Extra {
    #[bang]
    fn bang(&self) {
        self.obj.post(format!("inlet {}", self.obj.proxy_inlet()));
        self.bang.send().unwrap();
    }
}
impl Drop for Extra {
    fn drop(&mut self) {
        DROPS.fetch_add(1, Ordering::SeqCst);
    }
}

#[derive(MaxObject)]
#[max(name = "jit.gl.iolets.test", kind = gl)]
struct GlExtra {
    obj: ObjRef,
    out: Outlet<List>,
}
impl Ob3d for GlExtra {
    fn new(b: &mut JitBuilder<Self>) -> Result<Self, NewError> {
        assert_eq!(b.inlet(Inlet::proxy().hot().assist("control"))?.index(), 1);
        Ok(Self {
            obj: b.max_obj()?,
            out: b.outlet(Outlet::list().assist("custom list"))?,
        })
    }
    fn draw(&self, _: &DrawContext) -> JitResult<()> {
        Ok(())
    }
}
#[median::methods]
impl GlExtra {
    #[bang]
    fn bang(&self) {
        self.out
            .send(&[(self.obj.proxy_inlet() as i64).into()])
            .unwrap();
    }
}

#[derive(MaxObject)]
#[max(name = "jit.variable.iolets.test", kind = matrix_op, no_methods)]
struct Variable;
impl MatrixOp for Variable {
    fn io_count() -> IOCount {
        IOCount {
            inputs: Count::Variable,
            outputs: Count::Variable,
        }
    }
    fn new(b: &mut JitBuilder<Self>) -> Result<Self, NewError> {
        b.inlet(Inlet::proxy())?;
        Ok(Self)
    }
    fn calc(&self, _: &mut [Matrix<'_>], _: &mut [Matrix<'_>]) -> JitResult<()> {
        Ok(())
    }
}

#[test]
fn custom_matrix_iolets_follow_sdk_positions_and_dispatch_to_the_core() {
    let k = median_mock::lock();
    Extra::register();
    let object = k.new_object("jit.iolets.test", &[]);
    assert!(!object.is_null(), "{:?}", k.console_log());
    let inlets = k.inlets(object);
    assert_eq!(
        &inlets[1..],
        &[
            InletKind::Int(1),
            InletKind::Float(1),
            InletKind::Proxy { id: 4, stuffloc: 0 }
        ]
    );
    assert_eq!(inlets.len(), 4);
    assert_eq!(
        k.outlet_types(object),
        [
            Some("jit_matrix".into()),
            Some("jit_matrix".into()),
            None,
            Some("int".into()),
            Some("bang".into())
        ]
    );
    assert_eq!(
        k.assist(object, false, 1).unwrap(),
        "inlet 1: Jitter matrix"
    );
    assert_eq!(k.assist(object, false, 2).unwrap(), "integer");
    assert_eq!(k.assist(object, false, 3).unwrap(), "float");
    assert_eq!(k.assist(object, false, 4).unwrap(), "messages");
    assert_eq!(
        k.assist(object, true, 2).unwrap(),
        "outlet 2: Jitter matrix"
    );
    assert_eq!(k.assist(object, true, 3).unwrap(), "result");
    assert_eq!(k.assist(object, true, 4).unwrap(), "done");
    assert_eq!(
        (0..5)
            .map(|i| k.inlet_is_hot(object, i).unwrap())
            .collect::<Vec<_>>(),
        [true, false, false, true, false]
    );
    median_mock::jit::take_inletinfo_calls();
    let (method, _) = k.method(object, "inletinfo").unwrap();
    let method: unsafe extern "C" fn(
        *mut core::ffi::c_void,
        *mut core::ffi::c_void,
        core::ffi::c_long,
        *mut core::ffi::c_char,
    ) = unsafe { std::mem::transmute(method) };
    let box_ptr = std::ptr::dangling_mut::<core::ffi::c_void>();
    let mut cold = 0;
    unsafe {
        method(object, box_ptr, 1, &mut cold);
    }
    assert_eq!(cold, 1);
    assert_eq!(
        median_mock::jit::take_inletinfo_calls(),
        [(object as usize, box_ptr as usize, 1)]
    );
    unsafe {
        method(object, box_ptr, 3, &mut cold);
    }
    assert_eq!(cold, 0);
    assert!(median_mock::jit::take_inletinfo_calls().is_empty());
    k.send_inlet(object, 2, "int", &[MockAtom::Long(19)])
        .unwrap();
    k.send_inlet(object, 3, "float", &[MockAtom::Float(2.5)])
        .unwrap();
    k.send_inlet(object, 4, "bang", &[]).unwrap();
    let output = k.take_outlet_log();
    assert_eq!(
        (output[0].index, &output[0].atoms),
        (3, &vec![MockAtom::Long(19)])
    );
    assert_eq!(output[1].index, 4);
    let posts: Vec<_> = k.take_console_log().into_iter().map(|e| e.text).collect();
    assert_eq!(posts, ["float 2.5", "inlet 4"]);
    k.send_inlet(object, 3, "float", &[MockAtom::Float(-1.)])
        .unwrap();
    assert!(
        k.take_console_log()
            .iter()
            .any(|e| e.text.contains("inlet panic"))
    );
    let before = DROPS.load(Ordering::SeqCst);
    assert!(k.free_object(object));
    assert_eq!(DROPS.load(Ordering::SeqCst), before + 1);
    assert_eq!(k.live_object_count(), 0);
    assert_eq!(median_mock::jit::live_mop_count(), 0);
    assert!(
        k.take_console_log().is_empty(),
        "proxies must be freed before owner"
    );
}

#[test]
fn gl_extras_follow_matrix_and_dumpout_and_free_cleanly() {
    let k = median_mock::lock();
    GlExtra::register();
    let object = k.new_object("jit.gl.iolets.test", &["context".into()]);
    assert!(!object.is_null(), "{:?}", k.console_log());
    assert_eq!(
        k.outlet_types(object),
        [Some("jit_matrix".into()), None, Some("list".into())]
    );
    assert_eq!(k.assist(object, true, 2).unwrap(), "custom list");
    assert_eq!(k.assist(object, false, 1).unwrap(), "control");
    assert_eq!(k.inlet_is_hot(object, 1), Some(true));
    k.send_inlet(object, 1, "bang", &[]).unwrap();
    let output = k.take_outlet_log();
    assert_eq!(output[0].index, 2);
    assert_eq!(output[0].atoms, [MockAtom::Long(1)]);
    assert!(k.free_object(object));
    assert_eq!(median_mock::jit::context_count(), 0);
    assert!(k.take_console_log().is_empty());
}

#[test]
fn refusal_panics_invalid_descriptions_and_standalone_cores_release_resources() {
    let k = median_mock::lock();
    Extra::register();
    Variable::register();
    for mode in 1..=4 {
        let object = k.new_object("jit.iolets.test", &[MockAtom::Long(mode)]);
        assert!(object.is_null());
        assert_eq!(k.live_object_count(), 0);
        let errors = k.take_console_log();
        assert!(!errors.is_empty());
        assert!(errors.iter().all(|e| !e.text.contains("was not freed") && !e.text.contains("not a live object")), "{errors:?}");
    }
    // Custom proxies already exist when SDK setup fails after creating its dumpout.
    median_mock::jit::fail_next_mop_setup();
    assert!(k.new_object("jit.iolets.test", &[]).is_null());
    assert_eq!(k.live_object_count(), 0);
    assert_eq!(median_mock::jit::live_mop_count(), 0);
    assert!(
        k.take_console_log()
            .iter()
            .all(|e| !e.text.contains("was not freed") && !e.text.contains("without setup"))
    );
    assert!(k.new_object("jit.variable.iolets.test", &[]).is_null());
    assert!(
        k.take_console_log()
            .iter()
            .any(|e| e.text.contains("fixed matrix counts"))
    );
    unsafe {
        assert!(
            max_sys::jit_object_new(sym!("median_jit.iolets.test").as_ptr(), sym!("").as_ptr())
                .is_null()
        );
    }
    assert_eq!(k.live_object_count(), 0);
    assert!(!k.take_console_log().is_empty());
}
