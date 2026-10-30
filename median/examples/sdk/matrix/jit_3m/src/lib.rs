//! A port of the SDK's `matrix/jit.3m`: reports the minimum, mean and maximum of each plane of a
//! matrix.
//!
//! The C replaces the MOP's matrix processing (`max_jit_classex_mop_mproc`) to output the three
//! lists after `matrix_calc`; median's Max side has no such hook, so `calc` sends them itself, on
//! the thread the matrix arrived on, as the C's processing step does. Deviations:
//! - the lists come out of outlets 1 to 3, right of dumpout (outlet 0); the C puts them left of it;
//! - `bang` outputs whatever the MOP's output mode is (the C checks `outputmode`), and nothing
//!   before the first matrix (the C reads an unset atom); that `#[bang]` replaces the MOP's own
//!   `bang` on the Max class is untested;
//! - `min`, `mean` and `max` are read-only attributes with accessors written here (median has no
//!   array attributes), without `basic`;
//! - `calc` is serial (the C used the parallel calc), and float32 sums are accumulated as f64;
//! - the getters run on the caller's thread (the C's defer them, `JIT_ATTR_GET_DEFER_LOW`).
//!
//! `calc` is tested by calling it directly; the `jit_matrix` path in Max is untested.

use std::{
    ffi::{c_long, c_void},
    sync::Mutex,
};

use median::{
    attr::{AttrBuilder, AttrTrampGetMethod, AttrType},
    error::MaxError,
    jit::{
        JitBuilder, JitResult, MatrixOp,
        matrix::{Count, IOCount, Matrix, MatrixData, MatrixDataType, iter::JitEntryType},
        wrapper::{Core, MatrixKind},
    },
    max_sys::{self, t_jit_error_code as err, t_max_err},
    prelude::*,
    wrapper::WrapperWrapped,
};

#[derive(Clone, Copy, Debug, PartialEq)]
enum Value {
    Int(t_atom_long),
    Float(f64),
}

impl From<Value> for Atom {
    fn from(v: Value) -> Self {
        match v {
            Value::Int(i) => i.into(),
            Value::Float(f) => f.into(),
        }
    }
}

/// One entry per plane of the last matrix; empty before the first.
#[derive(Clone, Default)]
struct Stats {
    min: Vec<Value>,
    mean: Vec<Value>,
    max: Vec<Value>,
}

struct Outlets {
    min: Outlet<Any>,
    mean: Outlet<Any>,
    max: Outlet<Any>,
}

#[derive(JitMatrixObj)]
#[max(name = "jit.median.3m")]
pub struct Jit3m {
    stats: Mutex<Stats>,
    /// `None` for a core made without a Max object (from JavaScript, say).
    outlets: Option<Outlets>,
}

impl MatrixOp for Jit3m {
    fn new(b: &mut JitBuilder<Self>) -> Result<Self, NewError> {
        let outlets = match b.outlet(Outlet::any().assist("(list) min")) {
            Ok(min) => Some(Outlets {
                min,
                mean: b.outlet(Outlet::any().assist("(list) mean"))?,
                max: b.outlet(Outlet::any().assist("(list) max"))?,
            }),
            Err(_) => None,
        };
        Ok(Self {
            stats: Mutex::new(Stats::default()),
            outlets,
        })
    }

    fn io_count() -> IOCount {
        IOCount {
            inputs: Count::Fixed(1),
            outputs: Count::Fixed(0),
        }
    }

    fn class_setup(class: &mut Class<Core<Self, MatrixKind>>) -> MaxResult<()> {
        let attrs: [(&str, &'static std::ffi::CStr, AttrTrampGetMethod<_>); 3] = [
            ("min", c"Minimum", get_stat::<0>),
            ("mean", c"Mean", get_stat::<1>),
            ("max", c"Maximum", get_stat::<2>),
        ];
        for (name, label, get) in attrs {
            // GAP: CLASS_ATTR_SYM_VARSIZE (array attributes)
            let mut b = AttrBuilder::new_get(name, AttrType::AtomPtr, get);
            b.label(label).category(c"Value");
            class.add_attribute(b.build().map_err(|_| MaxError::Generic)?)?;
        }
        Ok(())
    }

    fn calc(&self, inputs: &mut [Matrix<'_>], _: &mut [Matrix<'_>]) -> JitResult<()> {
        let input = inputs.first_mut().ok_or(err::JIT_ERR_INVALID_PTR)?;
        let stats = {
            let mut lock = input.lock();
            let Ok(data) = lock.data() else {
                *self.stats.lock().unwrap() = Stats::default();
                return Err(err::JIT_ERR_INVALID_INPUT);
            };
            match data.info().kind()? {
                MatrixDataType::Char => stats::<u8>(&data, true, false)?,
                MatrixDataType::Int32 => stats::<i32>(&data, true, false)?,
                MatrixDataType::Float32 => stats::<f32>(&data, false, true)?,
                MatrixDataType::Float64 => stats::<f64>(&data, false, false)?,
            }
        };
        *self.stats.lock().unwrap() = stats;
        self.output();
        Ok(())
    }
}

/// Per plane statistics of `data`'s cells; min and max are ints for an integer matrix, and the
/// mean is rounded to f32 for a float32 one, as the C stores them.
fn stats<T: JitEntryType + Into<f64>>(
    data: &MatrixData<'_>,
    integer: bool,
    single: bool,
) -> JitResult<Stats> {
    let planes = data.info().planes()?;
    let mut min = vec![f64::INFINITY; planes];
    let mut max = vec![f64::NEG_INFINITY; planes];
    let mut sum = vec![0f64; planes];
    let mut count = 0usize;
    // SAFETY: the input stays locked and unwritten for this matrix_calc call.
    for chunk in unsafe { data.chunks::<T>()? } {
        for cell in chunk.entry_iter() {
            for (p, &v) in cell.iter().enumerate() {
                let v: f64 = v.into();
                min[p] = min[p].min(v);
                max[p] = max[p].max(v);
                sum[p] += v;
            }
            count += 1;
        }
    }
    if count == 0 {
        return Err(err::JIT_ERR_INVALID_INPUT);
    }
    let bound = |v: f64| {
        if integer {
            Value::Int(v as t_atom_long)
        } else {
            Value::Float(v)
        }
    };
    Ok(Stats {
        min: min.into_iter().map(bound).collect(),
        mean: sum
            .into_iter()
            .map(|s| {
                let m = s / count as f64;
                Value::Float(if single { m as f32 as f64 } else { m })
            })
            .collect(),
        max: max.into_iter().map(bound).collect(),
    })
}

fn send(outlet: &Outlet<Any>, values: &[Value]) {
    let _ = match values {
        [Value::Int(i)] => outlet.send_int(*i),
        [Value::Float(f)] => outlet.send_float(*f),
        _ => {
            let atoms: Vec<Atom> = values.iter().map(|&v| v.into()).collect();
            outlet.send(&sym!("list"), &atoms)
        }
    };
}

#[median::methods]
impl Jit3m {
    #[bang]
    pub fn output(&self) {
        let Some(o) = &self.outlets else { return };
        let stats = self.stats.lock().unwrap().clone();
        if stats.min.is_empty() {
            return;
        }
        send(&o.max, &stats.max);
        send(&o.mean, &stats.mean);
        send(&o.min, &stats.min);
    }
}

/// The getter of `min` (`WHICH` 0), `mean` (1) or `max` (2).
///
/// # Safety
/// Only Jitter calls it, as an attribute getter, with the `ac` and `av` it passes getters.
unsafe extern "C" fn get_stat<const WHICH: u8>(
    w: &Core<Jit3m, MatrixKind>,
    _attr: *mut c_void,
    ac: *mut c_long,
    av: *mut *mut max_sys::t_atom,
) -> t_max_err {
    median::ffi::guard_or(w, "3m getter", err::JIT_ERR_GENERIC as _, || {
        let stats = w.wrapped().stats.lock().unwrap().clone();
        let values = match WHICH {
            0 => stats.min,
            1 => stats.mean,
            _ => stats.max,
        };
        // SAFETY: the getter's own `ac` and `av`
        unsafe { write_atoms(ac, av, values.into_iter().map(Atom::from).collect()) }
    })
}

/// Fill a Jitter getter's results: as many atoms as fit into an array the caller passes, or all
/// of them into a new one the caller frees.
///
/// # Safety
/// `ac` and `av` are null or the out-parameters Jitter passes to an attribute getter; a non-null
/// `*av` holds `*ac` atoms.
unsafe fn write_atoms(
    ac: *mut c_long,
    av: *mut *mut max_sys::t_atom,
    atoms: Vec<Atom>,
) -> t_max_err {
    if ac.is_null() || av.is_null() {
        return 0;
    }
    // SAFETY: the caller's contract; `Atom` is a transparent `t_atom`
    unsafe {
        if (*av).is_null() || *ac <= 0 {
            let size = atoms.len().max(1) * std::mem::size_of::<max_sys::t_atom>();
            // GAP: Custom attribute accessors
            let p = max_sys::jit_getbytes(size as c_long).cast::<max_sys::t_atom>();
            if p.is_null() {
                *ac = 0;
                return err::JIT_ERR_OUT_OF_MEM as _;
            }
            *av = p;
            *ac = atoms.len() as c_long;
        } else {
            *ac = (*ac).min(atoms.len() as c_long);
        }
        for (i, atom) in atoms.into_iter().take(*ac as usize).enumerate() {
            (*av).add(i).cast::<Atom>().write(atom);
        }
    }
    0
}

median::ext_main! { Jit3m }

#[cfg(test)]
mod tests {
    use core::ffi::c_void;
    use median::jit::matrix::{MatrixDataType, MatrixInfo};
    use median_mock::{
        Atom,
        jit::{MatrixFixture, MatrixList},
    };

    type Calc =
        unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> median::max_sys::t_jit_err;

    fn setup(k: &median_mock::Kernel) -> (*mut c_void, *mut c_void, Calc) {
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object("jit.median.3m", &[]);
        assert!(!x.is_null(), "{:?}", k.console_log());
        let core = median_mock::jit::object_for(x);
        let (f, _) = k.method(core, "matrix_calc").unwrap();
        // SAFETY: `matrix_calc` is registered with this signature
        (x, core, unsafe { std::mem::transmute::<usize, Calc>(f) })
    }

    fn run(calc: Calc, core: *mut c_void, m: &MatrixFixture) -> median::max_sys::t_jit_err {
        let ins = MatrixList::new(&[m]);
        let outs = MatrixList::new(&[]);
        // SAFETY: live core and lists
        unsafe { calc(core, ins.as_ptr(), outs.as_ptr()) }
    }

    fn fixture(kind: MatrixDataType, planes: usize, dims: &[usize], bytes: &[u8]) -> MatrixFixture {
        let info = MatrixInfo::new(kind, planes, dims).unwrap();
        let mut m = MatrixFixture::new(*info.as_raw());
        m.write_bytes(bytes);
        m
    }

    fn outputs(k: &median_mock::Kernel) -> Vec<(usize, String, Vec<Atom>)> {
        k.take_outlet_log()
            .into_iter()
            .map(|e| (e.index, e.selector, e.atoms))
            .collect()
    }

    #[test]
    fn char_planes_go_out_max_mean_min_and_bang_repeats() {
        let k = median_mock::lock();
        let (x, core, calc) = setup(&k);
        assert_eq!(k.outlet_types(x).len(), 4);
        assert_eq!(k.assist(x, true, 1).unwrap(), "(list) min");
        assert_eq!(k.assist(x, true, 2).unwrap(), "(list) mean");
        assert_eq!(k.assist(x, true, 3).unwrap(), "(list) max");
        k.send(x, "bang", &[]).unwrap();
        assert!(k.take_outlet_log().is_empty());
        assert_eq!(k.attr_get(x, "min").unwrap(), []);

        let m = fixture(
            MatrixDataType::Char,
            2,
            &[2, 2],
            &[1, 10, 5, 20, 3, 30, 7, 40],
        );
        assert_eq!(run(calc, core, &m), 0);
        assert_eq!(m.lock_state(), 0);
        let expected = vec![
            (3, "list".into(), vec![Atom::Long(7), Atom::Long(40)]),
            (2, "list".into(), vec![Atom::Float(4.0), Atom::Float(25.0)]),
            (1, "list".into(), vec![Atom::Long(1), Atom::Long(10)]),
        ];
        assert_eq!(outputs(&k), expected);
        k.send(x, "bang", &[]).unwrap();
        assert_eq!(outputs(&k), expected);
        assert_eq!(
            k.attr_get(x, "max").unwrap(),
            [Atom::Long(7), Atom::Long(40)]
        );
        assert_eq!(
            k.attr_get(x, "mean").unwrap(),
            [Atom::Float(4.0), Atom::Float(25.0)]
        );
        assert!(k.free_object(x));
    }

    #[test]
    fn single_plane_sends_scalars_and_floats_stay_floats() {
        let k = median_mock::lock();
        let (x, core, calc) = setup(&k);
        let bytes: Vec<u8> = [-1i32, 4, 9].iter().flat_map(|v| v.to_ne_bytes()).collect();
        let m = fixture(MatrixDataType::Int32, 1, &[3], &bytes);
        assert_eq!(run(calc, core, &m), 0);
        assert_eq!(
            outputs(&k),
            [
                (3, "int".into(), vec![Atom::Long(9)]),
                (2, "float".into(), vec![Atom::Float(4.0)]),
                (1, "int".into(), vec![Atom::Long(-1)]),
            ]
        );
        let bytes: Vec<u8> = [0.5f64, -2.0, 1.0, 4.5, 3.0, 0.0]
            .iter()
            .flat_map(|v| v.to_ne_bytes())
            .collect();
        let m = fixture(MatrixDataType::Float64, 1, &[1, 2, 3], &bytes);
        assert_eq!(run(calc, core, &m), 0);
        assert_eq!(
            outputs(&k),
            [
                (3, "float".into(), vec![Atom::Float(4.5)]),
                (2, "float".into(), vec![Atom::Float(7.0 / 6.0)]),
                (1, "float".into(), vec![Atom::Float(-2.0)]),
            ]
        );
        assert!(k.free_object(x));
    }

    #[test]
    fn an_empty_input_list_is_an_error() {
        let k = median_mock::lock();
        let (x, core, calc) = setup(&k);
        let m = fixture(MatrixDataType::Char, 1, &[2], &[1, 2]);
        assert_eq!(run(calc, core, &m), 0);
        k.take_outlet_log();
        let ins = MatrixList::new(&[]);
        // SAFETY: live core and list
        let code = unsafe { calc(core, ins.as_ptr(), ins.as_ptr()) };
        assert_eq!(
            code,
            median::max_sys::t_jit_error_code::JIT_ERR_INVALID_PTR as median::max_sys::t_jit_err
        );
        assert!(k.take_outlet_log().is_empty());
        assert!(k.free_object(x));
    }
}
