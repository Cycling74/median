//! A port of the SDK's `matrix/jit.eclipse`: two-source meta-imaging on 4-plane char matrices.
//! The left image is tiled `rows` by `columns` times, and each tile is tinted toward the cell of
//! the right image at the tile's position (added or multiplied, `op`), inverted where that cell
//! is darker than `thresh` (flipped for a negative `thresh`).
//!
//! Every 2D section of the left image is tinted from the right image's first section, as in the
//! C. The C calculates serially, as the port does over median's chunk views. Deviations: the right
//! image is read at the tile position clamped to its size (the C reads past its end when it is
//! smaller than the left one), and each output cell keeps its own input alpha (the C copies the
//! alpha of the first cell of the input row, an evident slip). The attributes are not basic.

use median::{
    jit::{
        JitBuilder, JitObjRef, JitResult, MatrixOp,
        matrix::{Count, IOCount, MOP, Matrix, MatrixData, MatrixDataType, iter::JitEntryType},
        wrapper::{Core, MatrixKind},
    },
    max_sys::t_jit_error_code as err,
    prelude::*,
};

#[derive(JitMatrixObj)]
#[max(name = "jit.median.eclipse")]
pub struct Eclipse {
    obj: JitObjRef,
    #[attr(
        label = "Eclipse Mode",
        category = "Behavior",
        enum = "\"Color Tint Color Frames\" \"Color Tint Monochrome Frames\" \"Monochrome Tint Color Frames\" \"Monochrome Tint Monochrome Frames\""
    )]
    mode: Int64,
    #[attr(label = "Rows", category = "Behavior")]
    rows: Int64,
    #[attr(label = "Columns", category = "Behavior")]
    columns: Int64,
    #[attr(label = "Red Tinting", category = "Behavior")]
    red: Float32,
    #[attr(label = "Green Tinting", category = "Behavior")]
    green: Float32,
    #[attr(label = "Blue Tinting", category = "Behavior")]
    blue: Float32,
    #[attr(label = "Threshold", category = "Behavior")]
    thresh: Float32,
    #[attr(style = "onoff", label = "Tint", category = "Behavior")]
    tint: Int64,
    #[attr(style = "onoff", label = "Invert", category = "Behavior")]
    inv: Int64,
    #[attr(
        name = "op",
        label = "Operator",
        category = "Behavior",
        enum = "Add Multiply"
    )]
    oper: Int64,
}

impl MatrixOp for Eclipse {
    fn new(builder: &mut JitBuilder<Self>) -> Result<Self, NewError> {
        Ok(Self {
            obj: builder.obj(),
            mode: 0i64.into(),
            rows: 1i64.into(),
            columns: 1i64.into(),
            red: 0.0.into(),
            green: 0.0.into(),
            blue: 0.0.into(),
            thresh: (-1.01).into(),
            tint: 1i64.into(),
            inv: 0i64.into(),
            oper: 0i64.into(),
        })
    }

    fn io_count() -> IOCount {
        IOCount {
            inputs: Count::Fixed(2),
            outputs: Count::Fixed(1),
        }
    }

    fn mop_setup(mop: &mut MOP<'_>) -> JitResult<()> {
        mop.single_type(MatrixDataType::Char)?;
        mop.single_plane_count(4)
    }

    fn class_setup(_: &mut Class<Core<Self, MatrixKind>>) -> MaxResult<()> {
        post!("Eclipse, by R. Luke DuBois / Mark McNamara");
        post!(
            "Copyright (c) 2002 The Trustees of Columbia University in the City of New York / Cycling'74."
        );
        post!("All rights reserved.");
        Ok(())
    }

    fn calc(&self, inputs: &mut [Matrix<'_>], outputs: &mut [Matrix<'_>]) -> JitResult<()> {
        let output = outputs.first_mut().ok_or(err::JIT_ERR_INVALID_PTR)?;
        let [first, second] = inputs else {
            return Err(err::JIT_ERR_INVALID_PTR);
        };
        let out_kind = output.lock().info()?.kind()?;
        if first.lock().info()?.kind()? != MatrixDataType::Char || out_kind != MatrixDataType::Char
        {
            return Err(err::JIT_ERR_MISMATCH_TYPE);
        }
        let image = Cells::<u8>::read(first)?;
        if image.planes != 4 {
            return Err(err::JIT_ERR_MISMATCH_PLANE);
        }
        if second.lock().info()?.kind()? != MatrixDataType::Char {
            self.obj.post("type not ok");
            return Err(err::JIT_ERR_MISMATCH_TYPE);
        }
        let tints = Cells::<u8>::read(second)?;
        if tints.planes != 4 {
            self.obj.post("planes not ok");
            return Err(err::JIT_ERR_MISMATCH_PLANE);
        }
        let params = Params {
            mode: self.mode.get() as i64,
            color: [&self.red, &self.green, &self.blue].map(|v| (v.get() as f64 * 255.0) as i64),
            thresh: (self.thresh.get() as f64 * 255.0) as i64,
            tint: self.tint.get() != 0,
            inv: self.inv.get() != 0,
            rows: self.rows.get() as i64,
            columns: self.columns.get() as i64,
            multiply: self.oper.get() != 0,
        };
        Cells::update(output, |out| {
            if out.planes != 4 {
                return Err(err::JIT_ERR_MISMATCH_PLANE);
            }
            let dims: Vec<usize> = (0..out.dims.len())
                .map(|i| out.dims[i].min(image.dims.get(i).copied().unwrap_or(1)))
                .collect();
            let (width, height) = (dims[0], dims.get(1).copied().unwrap_or(1));
            let slices: usize = dims.iter().skip(2).product();
            if width == 0 || height == 0 {
                return Ok(());
            }
            for slice in 0..slices {
                let mut coord = [0; 32];
                let mut rest = slice;
                for (c, &d) in coord.iter_mut().zip(&dims).skip(2) {
                    *c = rest % d;
                    rest /= d;
                }
                params.slice(&image, &tints, out, coord, width, height);
            }
            Ok(())
        })
    }
}

#[median::methods]
impl Eclipse {}

median::ext_main! { Eclipse }

struct Params {
    mode: i64,
    color: [i64; 3],
    thresh: i64,
    tint: bool,
    inv: bool,
    rows: i64,
    columns: i64,
    multiply: bool,
}

impl Params {
    /// One 2D section, with the C's loop structure: every tile row `r` and column `c` writes
    /// every `rows`th input row and `columns`th input column, until it passes the output's edge.
    fn slice(
        &self,
        image: &Cells<u8>,
        tints: &Cells<u8>,
        out: &mut Cells<u8>,
        mut coord: [usize; 32],
        width: usize,
        height: usize,
    ) {
        let (width, height) = (width as i64, height as i64);
        let rows = self.rows.max(1).min(height);
        let columns = self.columns.max(1).min(width);
        let rowstep = ((height as f32 / rows as f32) as f64 + 0.5) as i64;
        let colstep = ((width as f32 / columns as f32) as f64 + 0.5) as i64;
        let offset = |n: i64, size: i64| {
            if n as f64 > size as f64 * 0.1 {
                (n as f64 - size as f64 * 0.1) as i64
            } else {
                1
            }
        };
        let (rowoffset, coloffset) = (offset(rows, height), offset(columns, width));
        let flip = self.thresh < 0;
        let thresh = self.thresh.abs();
        'section: for r in 0..=rows + rowoffset {
            let (mut i, mut i1) = (0, r * rowstep);
            while i < height {
                'row: for c in 0..=columns + coloffset {
                    let t = tint_cell(tints, r * rowstep, c * colstep);
                    let mut tint = if self.mode == 0 || self.mode == 1 {
                        [t[1], t[2], t[3]].map(i64::from)
                    } else {
                        [(t[1] as i64 + t[2] as i64 + t[3] as i64) / 3; 3]
                    };
                    let cut = (tint[0] + tint[1] + tint[2]) / 3 >= thresh;
                    if !self.tint {
                        tint = [0; 3];
                    }
                    let (mut j, mut j1) = (0, c * colstep);
                    while j < width {
                        if j1 >= width {
                            break 'row;
                        }
                        if i1 >= height {
                            break 'section;
                        }
                        coord[0] = j as usize;
                        coord[1] = i as usize;
                        let Some(source) = image.offset(&coord, false) else {
                            return;
                        };
                        coord[0] = j1 as usize;
                        coord[1] = i1 as usize;
                        let Some(target) = out.offset(&coord, false) else {
                            return;
                        };
                        let src = &image.data[source..source + 4];
                        let dst = &mut out.data[target..target + 4];
                        dst[0] = src[0];
                        let average = (src[1] as i64 + src[2] as i64 + src[3] as i64) / 3;
                        let direct = cut != flip;
                        for k in 0..3 {
                            let base = if self.mode == 0 || self.mode == 2 {
                                src[k + 1] as i64
                            } else {
                                average
                            };
                            let base = if direct { base } else { 255 - base };
                            let color = if !direct && self.inv {
                                -self.color[k]
                            } else {
                                self.color[k]
                            };
                            let value = if self.multiply {
                                (base * (tint[k] + color)) >> 8
                            } else {
                                base + tint[k] + color
                            };
                            dst[k + 1] = value.clamp(0, 255) as u8;
                        }
                        j += columns;
                        j1 += 1;
                    }
                }
                i += rows;
                i1 += 1;
            }
        }
    }
}

/// The tint image's cell at `row` and `column` of its first section, clamped to its size.
fn tint_cell(tints: &Cells<u8>, row: i64, column: i64) -> [u8; 4] {
    let mut coord = [0; 32];
    let last = |i: usize| tints.dims.get(i).copied().unwrap_or(1).saturating_sub(1) as i64;
    coord[0] = column.min(last(0)) as usize;
    coord[1] = row.min(last(1)) as usize;
    tints
        .offset(&coord, false)
        .map_or([0; 4], |o| [0, 1, 2, 3].map(|p| tints.data[o + p]))
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

    fn fixture(planes: usize, dims: &[usize], bytes: &[u8]) -> MatrixFixture {
        let info = MatrixInfo::new(MatrixDataType::Char, planes, dims).unwrap();
        let mut m = MatrixFixture::new(*info.as_raw());
        m.write_bytes(bytes);
        m
    }

    fn setup(k: &median_mock::Kernel, args: &[Atom]) -> (*mut c_void, *mut c_void, Calc) {
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let object = k.new_object("jit.median.eclipse", args);
        assert!(!object.is_null(), "{:?}", k.console_log());
        let core = median_mock::jit::object_for(object);
        let (f, _) = k.method(core, "matrix_calc").unwrap();
        // SAFETY: matrix_calc is registered as `t_jit_err f(x, inputs, outputs)`.
        (object, core, unsafe {
            std::mem::transmute::<usize, Calc>(f)
        })
    }

    fn run(
        calc: Calc,
        core: *mut c_void,
        inputs: [&MatrixFixture; 2],
        output: &MatrixFixture,
    ) -> i64 {
        let ins = MatrixList::new(&inputs);
        let outs = MatrixList::new(&[output]);
        // SAFETY: the core and lists are live for the call.
        unsafe { calc(core, ins.as_ptr(), outs.as_ptr()) }
    }

    fn pixel(y: usize, x: usize) -> [u8; 4] {
        [(y * 4 + x) as u8, (x * 10) as u8, (y * 10) as u8, 0]
    }

    #[test]
    fn tints_the_whole_image_by_default() {
        let k = median_mock::lock();
        let (object, core, calc) = setup(&k, &[]);
        let image = fixture(
            4,
            &[2, 2],
            &[9, 100, 200, 250, 8, 1, 2, 3, 7, 4, 5, 6, 6, 7, 8, 9],
        );
        let tints = fixture(
            4,
            &[2, 2],
            &[0, 10, 20, 30, 0, 99, 99, 99, 0, 99, 99, 99, 0, 99, 99, 99],
        );
        let output = fixture(4, &[2, 2], &[]);
        assert_eq!(run(calc, core, [&image, &tints], &output), 0);
        assert_eq!(
            output.bytes(),
            [
                9, 110, 220, 255, 8, 11, 22, 33, 7, 14, 25, 36, 6, 17, 28, 39
            ]
        );
        assert!(k.free_object(object));
    }

    #[test]
    fn tiles_rows_by_columns() {
        let k = median_mock::lock();
        let args = [
            "@rows".into(),
            2.into(),
            "@columns".into(),
            2.into(),
            "@tint".into(),
            0.into(),
        ];
        let (object, core, calc) = setup(&k, &args);
        let bytes: Vec<u8> = (0..16).flat_map(|n| pixel(n / 4, n % 4)).collect();
        let image = fixture(4, &[4, 4], &bytes);
        let tints = fixture(4, &[1], &[0; 4]);
        let output = fixture(4, &[4, 4], &[]);
        assert_eq!(run(calc, core, [&image, &tints], &output), 0);
        let expected: Vec<u8> = (0..16)
            .flat_map(|n| pixel(2 * (n / 4 % 2), 2 * (n % 4 % 2)))
            .collect();
        assert_eq!(output.bytes(), expected);
        assert!(k.free_object(object));
    }

    #[test]
    fn thresholds_invert_and_multiply() {
        let k = median_mock::lock();
        let (object, core, calc) =
            setup(&k, &["@thresh".into(), 0.5.into(), "@op".into(), 1.into()]);
        let image = fixture(4, &[1], &[7, 100, 50, 200]);
        let bright = fixture(4, &[1], &[0, 255, 255, 255]);
        let dark = fixture(4, &[1], &[0; 4]);
        let output = fixture(4, &[1], &[]);
        assert_eq!(run(calc, core, [&image, &bright], &output), 0);
        assert_eq!(output.bytes(), [7, 99, 49, 199]);
        assert_eq!(run(calc, core, [&image, &dark], &output), 0);
        assert_eq!(output.bytes(), [7, 0, 0, 0]);

        k.attr_set(object, "op", &[Atom::Long(0)]).unwrap();
        k.attr_set(object, "inv", &[Atom::Long(1)]).unwrap();
        k.attr_set(object, "red", &[Atom::Float(0.5)]).unwrap();
        assert_eq!(run(calc, core, [&image, &dark], &output), 0);
        assert_eq!(output.bytes(), [7, 28, 205, 55]);
        assert!(k.free_object(object));
    }

    #[test]
    fn checks_the_tint_image() {
        let k = median_mock::lock();
        let (object, core, calc) = setup(&k, &[]);
        k.take_console_log();
        let image = fixture(4, &[1], &[0; 4]);
        let tints = fixture(3, &[1], &[0; 3]);
        let output = fixture(4, &[1], &[]);
        assert_eq!(
            run(calc, core, [&image, &tints], &output),
            median::max_sys::t_jit_error_code::JIT_ERR_MISMATCH_PLANE as i64
        );
        let log = k.take_console_log();
        assert!(log.iter().any(|e| e.text == "planes not ok"), "{log:?}");
        assert!(k.free_object(object));
    }
}
