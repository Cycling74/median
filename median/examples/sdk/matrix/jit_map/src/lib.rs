//! A port of the SDK's `matrix/jit.map`: maps every cell of a matrix of any type linearly from
//! the range `map[0]..map[1]` to `map[2]..map[3]`, clipping to the output range when `clip` is on.
//!
//! The C calculates with `jit_parallel_ndim_simplecalc2`; the port runs serially over median's
//! chunk views, with the same domain: an input dimension of size 1 repeats across the output, a
//! smaller one leaves the rest of the output untouched, and output plane `p` reads input plane
//! `p % planes`. `map` is a four-element array attribute (`jit_attr_offset_array` in the C),
//! registered with accessors written here. The attributes are not basic.

use core::ffi::{c_long, c_void};
use median::{
    attr::{AttrBuilder, AttrType},
    jit::{
        JitBuilder, JitResult, MatrixOp,
        matrix::{Count, IOCount, Matrix, MatrixData, MatrixDataType, iter::JitEntryType},
        wrapper::{Core, MatrixKind},
    },
    max_sys::{self, e_max_errorcodes::*, t_atom, t_jit_error_code as err, t_max_err},
    prelude::*,
};

/// `jit.fixmath.h`'s `fixed1`: char cells are mapped in 16.16 fixed point.
const FIXED1: f64 = 65536.0;

#[derive(JitMatrixObj)]
#[max(name = "jit.median.map")]
pub struct Map {
    map: [Float64; 4],
    #[attr(style = "onoff", label = "Clip Values", category = "Behavior")]
    clip: Int64,
}

impl MatrixOp for Map {
    fn new(_: &mut JitBuilder<Self>) -> Result<Self, NewError> {
        Ok(Self {
            map: [0.0, 1.0, 0.0, 1.0].map(Float64::new),
            clip: 1i64.into(),
        })
    }

    fn io_count() -> IOCount {
        IOCount {
            inputs: Count::Fixed(1),
            outputs: Count::Fixed(1),
        }
    }

    fn class_setup(class: &mut Class<Core<Self, MatrixKind>>) -> MaxResult<()> {
        // GAP: CLASS_ATTR_SYM_VARSIZE (array attributes)
        let mut b = AttrBuilder::new_accessors("map", AttrType::Float64, map_get, map_set);
        b.label(c"Input to Output Map").category(c"Behavior");
        let attr = b.build().map_err(|_| median::error::MaxError::Generic)?;
        class.add_attribute(attr)
    }

    fn calc(&self, inputs: &mut [Matrix<'_>], outputs: &mut [Matrix<'_>]) -> JitResult<()> {
        let input = inputs.first_mut().ok_or(err::JIT_ERR_INVALID_PTR)?;
        let output = outputs.first_mut().ok_or(err::JIT_ERR_INVALID_PTR)?;
        let map = self.map.each_ref().map(Float64::get);
        let clipping = self.clip.get() != 0;
        let scale = (map[3] - map[2]) / (map[1] - map[0]);
        let bias = map[2] - map[0] * scale;
        let (min, max) = (map[2], map[3]);
        match common_kind(input, output)? {
            MatrixDataType::Char => {
                let (scale, bias) = ((scale * FIXED1) as i64, (bias * 255.0 * FIXED1) as i64);
                let (lo, hi) = (char_limit(min), char_limit(max));
                map_cells(input, output, |v: u8| {
                    let v = (v as i64 * scale + bias) >> 16;
                    // Unclipped, the C stores the low byte.
                    if clipping {
                        clip(v, lo, hi) as u8
                    } else {
                        v as u8
                    }
                })
            }
            MatrixDataType::Int32 => {
                let (lo, hi) = (min as i64, max as i64);
                map_cells(input, output, |v: i32| {
                    let v = v as f64 * scale + bias;
                    if clipping {
                        clip(v as i64, lo, hi) as i32
                    } else {
                        v as i32
                    }
                })
            }
            MatrixDataType::Float32 => {
                let (scale, bias, lo, hi) = (scale as f32, bias as f32, min as f32, max as f32);
                map_cells(input, output, |v: f32| {
                    let v = v * scale + bias;
                    if clipping { clip(v, lo, hi) } else { v }
                })
            }
            MatrixDataType::Float64 => map_cells(input, output, |v: f64| {
                let v = v * scale + bias;
                if clipping { clip(v, min, max) } else { v }
            }),
        }
    }
}

#[median::methods]
impl Map {}

median::ext_main! { Map }

fn char_limit(v: f64) -> i64 {
    ((v * 255.0) as i64).clamp(0, 255)
}

/// The C's `tmp>max?max:tmp<min?min:tmp`, which, unlike `clamp`, accepts `min > max`.
fn clip<T: PartialOrd>(v: T, min: T, max: T) -> T {
    if v > max {
        max
    } else if v < min {
        min
    } else {
        v
    }
}

type Wrapper = Core<Map, MatrixKind>;

/// The `map` getter: four float atoms, in an array allocated as Jitter expects if the one given
/// is too small.
///
/// # Safety
/// Only Max calls it, as an attribute getter, with the `ac` and `av` it passes getters.
unsafe extern "C" fn map_get(
    w: &Wrapper,
    _attr: *mut c_void,
    ac: *mut c_long,
    av: *mut *mut t_atom,
) -> t_max_err {
    median::ffi::guard_or(w, "map getter", MAX_ERR_GENERIC as _, || {
        if ac.is_null() || av.is_null() {
            return MAX_ERR_NONE as _;
        }
        let values = w.wrapped().map.each_ref().map(Float64::get);
        // SAFETY: `ac` and `av` are the getter's out-parameters; `*av` is null or holds `*ac`
        // atoms, and a new array is the caller's to free.
        unsafe {
            if (*av).is_null() || (*ac as usize) < values.len() {
                // GAP: jit_getbytes
                let p = max_sys::jit_getbytes((values.len() * size_of::<t_atom>()) as c_long);
                if p.is_null() {
                    *ac = 0;
                    return MAX_ERR_OUT_OF_MEM as _;
                }
                *av = p.cast();
            }
            *ac = values.len() as c_long;
            for (i, v) in values.into_iter().enumerate() {
                (*av).add(i).cast::<Atom>().write(Atom::from(v));
            }
        }
        MAX_ERR_NONE as _
    })
}

/// The `map` setter: sets as many leading elements as it is given atoms. The C passes `0` as the
/// array's count offset (marked "fix"), which would store the count over the object header; the
/// port keeps the intended fixed size of four.
///
/// # Safety
/// Only Max calls it, as an attribute setter, with `av` holding `ac` atoms.
unsafe extern "C" fn map_set(
    w: &Wrapper,
    _attr: *mut c_void,
    ac: c_long,
    av: *mut t_atom,
) -> t_max_err {
    median::ffi::guard_or(w, "map setter", MAX_ERR_GENERIC as _, || {
        let atoms: &[Atom] = if ac > 0 && !av.is_null() {
            // SAFETY: Max passes `ac` valid atoms; `Atom` is a transparent `t_atom`.
            unsafe { std::slice::from_raw_parts(av.cast(), ac as usize) }
        } else {
            &[]
        };
        for (field, atom) in w.wrapped().map.iter().zip(atoms) {
            field.set(atom.get_float());
        }
        MAX_ERR_NONE as _
    })
}

fn common_kind(input: &mut Matrix<'_>, output: &mut Matrix<'_>) -> JitResult<MatrixDataType> {
    let kind = input.lock().info()?.kind()?;
    if output.lock().info()?.kind()? != kind {
        return Err(err::JIT_ERR_MISMATCH_TYPE);
    }
    Ok(kind)
}

/// Write `f` of each input cell to the output cells it covers.
fn map_cells<T: JitEntryType>(
    input: &mut Matrix<'_>,
    output: &mut Matrix<'_>,
    f: impl Fn(T) -> T,
) -> JitResult<()> {
    let input = Cells::<T>::read(input)?;
    Cells::update(output, |out| {
        for cell in 0..out.cell_count() {
            let Some(source) = input.offset(&out.coord(cell), true) else {
                continue;
            };
            for plane in 0..out.planes {
                out.data[cell * out.planes + plane] = f(input.data[source + plane % input.planes]);
            }
        }
        Ok(())
    })
}

/// A copy of a matrix's cells, planes interleaved and dimension 0 fastest.
struct Cells<T> {
    dims: Vec<usize>,
    planes: usize,
    data: Vec<T>,
}

impl<T: JitEntryType> Cells<T> {
    fn read(matrix: &mut Matrix<'_>) -> JitResult<Self> {
        let mut lock = matrix.lock();
        Self::copy(&lock.data()?)
    }

    /// Copy the matrix, let `f` change the copy, and write it back.
    fn update(
        matrix: &mut Matrix<'_>,
        f: impl FnOnce(&mut Self) -> JitResult<()>,
    ) -> JitResult<()> {
        let mut lock = matrix.lock();
        let mut data = lock.data()?;
        let mut cells = Self::copy(&data)?;
        f(&mut cells)?;
        let mut source = cells.data.chunks_exact(cells.planes);
        // SAFETY: the output belongs to this matrix_calc call, and every input was copied (its
        // borrow ended) before, so an in-place output aliases no live reference.
        for mut chunk in unsafe { data.chunks_mut::<T>()? } {
            for cell in chunk.entry_iter_mut() {
                cell.copy_from_slice(source.next().ok_or(err::JIT_ERR_OUT_OF_BOUNDS)?);
            }
        }
        Ok(())
    }

    fn copy(data: &MatrixData<'_>) -> JitResult<Self> {
        let info = data.info();
        let dims = info.dimensions()?.iter().map(|&d| d as usize).collect();
        let planes = info.planes()?;
        let mut cells = Vec::new();
        // SAFETY: matrix_calc's caller keeps the locked matrix initialized and unwritten while
        // it runs, and the borrow ends with this function.
        for chunk in unsafe { data.chunks::<T>()? } {
            for cell in chunk.entry_iter() {
                cells.extend_from_slice(cell);
            }
        }
        Ok(Self {
            dims,
            planes,
            data: cells,
        })
    }

    fn cell_count(&self) -> usize {
        self.data.len() / self.planes
    }

    fn coord(&self, mut cell: usize) -> [usize; 32] {
        let mut coord = [0; 32];
        for (c, &d) in coord.iter_mut().zip(&self.dims) {
            *c = cell % d;
            cell /= d;
        }
        coord
    }

    /// The index of the first plane of the cell at `coord`, or `None` outside the matrix. With
    /// `repeat`, a dimension of size 1 (or one the matrix lacks) matches every coordinate.
    fn offset(&self, coord: &[usize; 32], repeat: bool) -> Option<usize> {
        let (mut offset, mut stride) = (0, self.planes);
        for (i, &c) in coord.iter().enumerate() {
            let d = self.dims.get(i).copied().unwrap_or(1);
            let c = if repeat && d == 1 { 0 } else { c };
            if c >= d {
                return None;
            }
            offset += c * stride;
            stride *= d;
        }
        Some(offset)
    }
}

#[cfg(test)]
mod tests {
    use core::ffi::c_void;
    use median::jit::matrix::{MatrixDataType, MatrixInfo};
    use median_mock::{
        Atom,
        jit::{MatrixFixture, MatrixList},
    };

    type Calc = unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> i64;

    fn fixture(kind: MatrixDataType, planes: usize, dims: &[usize], bytes: &[u8]) -> MatrixFixture {
        let mut m = MatrixFixture::new(*MatrixInfo::new(kind, planes, dims).unwrap().as_raw());
        m.write_bytes(bytes);
        m
    }

    fn setup(k: &median_mock::Kernel, args: &[Atom]) -> (*mut c_void, *mut c_void, Calc) {
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let object = k.new_object("jit.median.map", args);
        assert!(!object.is_null(), "{:?}", k.console_log());
        let core = median_mock::jit::object_for(object);
        let (f, _) = k.method(core, "matrix_calc").unwrap();
        // SAFETY: matrix_calc is registered as `t_jit_err f(x, inputs, outputs)`.
        (object, core, unsafe {
            std::mem::transmute::<usize, Calc>(f)
        })
    }

    fn run(calc: Calc, core: *mut c_void, input: &MatrixFixture, output: &MatrixFixture) -> i64 {
        let ins = MatrixList::new(&[input]);
        let outs = MatrixList::new(&[output]);
        // SAFETY: the core and lists are live for the call.
        unsafe { calc(core, ins.as_ptr(), outs.as_ptr()) }
    }

    fn f64_bytes(values: &[f64]) -> Vec<u8> {
        values.iter().flat_map(|v| v.to_ne_bytes()).collect()
    }

    fn f64_atoms(values: &[f64]) -> Vec<Atom> {
        values.iter().map(|&v| Atom::Float(v)).collect()
    }

    #[test]
    fn map_is_a_four_element_array_attribute() {
        let k = median_mock::lock();
        let (object, _, _) = setup(&k, &["@map".into(), 0.0.into(), 2.0.into()]);
        assert_eq!(
            k.attr_get(object, "map").unwrap(),
            f64_atoms(&[0.0, 2.0, 0.0, 1.0])
        );
        k.attr_set(object, "map", &f64_atoms(&[1.0, 2.0, 3.0, 4.0, 5.0]))
            .unwrap();
        assert_eq!(
            k.attr_get(object, "map").unwrap(),
            f64_atoms(&[1.0, 2.0, 3.0, 4.0])
        );
        assert_eq!(k.attr_get(object, "clip").unwrap(), [Atom::Long(1)]);
        assert!(k.free_object(object));
    }

    #[test]
    fn maps_char_in_fixed_point() {
        let k = median_mock::lock();
        let (object, core, calc) = setup(
            &k,
            &[
                "@map".into(),
                0.0.into(),
                1.0.into(),
                1.0.into(),
                0.0.into(),
                "@clip".into(),
                0.into(),
            ],
        );
        let input = fixture(MatrixDataType::Char, 2, &[2], &[0, 10, 200, 255]);
        let output = fixture(MatrixDataType::Char, 2, &[2], &[]);
        assert_eq!(run(calc, core, &input, &output), 0);
        assert_eq!(output.bytes(), [255, 245, 55, 0]);

        // Clipping to an inverted range follows the C's comparison order.
        k.attr_set(object, "clip", &[Atom::Long(1)]).unwrap();
        assert_eq!(run(calc, core, &input, &output), 0);
        assert_eq!(output.bytes(), [0, 0, 0, 255]);

        k.attr_set(object, "map", &f64_atoms(&[0.0, 1.0, 0.0, 0.5]))
            .unwrap();
        assert_eq!(run(calc, core, &input, &output), 0);
        assert_eq!(output.bytes(), [0, 5, 100, 127]);
        assert!(k.free_object(object));
    }

    #[test]
    fn maps_floats_and_longs_with_and_without_clip() {
        let k = median_mock::lock();
        let (object, core, calc) = setup(
            &k,
            &[
                "@map".into(),
                0.0.into(),
                2.0.into(),
                10.0.into(),
                20.0.into(),
            ],
        );
        let input = fixture(
            MatrixDataType::Float64,
            1,
            &[3],
            &f64_bytes(&[-1.0, 1.0, 4.0]),
        );
        let output = fixture(MatrixDataType::Float64, 1, &[3], &[]);
        assert_eq!(run(calc, core, &input, &output), 0);
        assert_eq!(output.bytes(), f64_bytes(&[10.0, 15.0, 20.0]));
        k.attr_set(object, "clip", &[Atom::Long(0)]).unwrap();
        assert_eq!(run(calc, core, &input, &output), 0);
        assert_eq!(output.bytes(), f64_bytes(&[5.0, 15.0, 30.0]));

        let longs: Vec<u8> = [-1i32, 1, 3].iter().flat_map(|v| v.to_ne_bytes()).collect();
        let input = fixture(MatrixDataType::Int32, 1, &[3], &longs);
        let output = fixture(MatrixDataType::Int32, 1, &[3], &[]);
        assert_eq!(run(calc, core, &input, &output), 0);
        let expected: Vec<u8> = [5i32, 15, 25]
            .iter()
            .flat_map(|v| v.to_ne_bytes())
            .collect();
        assert_eq!(output.bytes(), expected);
        assert!(k.free_object(object));
    }
}
