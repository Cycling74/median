//! A port of the SDK's `gl/jit.gl.gridshape`: draws a sphere, torus, cylinder, open cylinder,
//! cube, open cube, plane, circle, cone or capsule as a grid of `dim` vertices.
//!
//! The geometry is the C's. Deviations:
//! - the grid is drawn with `jit_ob3d_draw_chunk`, with `gridmode` choosing a `quad_grid` or
//!   `tri_grid` chunk. The C hands it to an internal `jit.gl.mesh` (with a tangent matrix it
//!   computes alongside) unless `cache_mode` is `immediate`, and then draws it itself with
//!   `glBegin`/`glVertex3f` or a display list; median has no raw OpenGL, and the mesh, the
//!   tangents and the `get_cache` method that hands out the mesh's cache are left out.
//!   `cache_mode` and `displaylist` are kept as attributes that only trigger a recalculation;
//! - the geometry is kept in a `Vec` and copied into a chunk made on every draw, since median's
//!   `GLChunk` lives only inside a GL callback;
//! - the `multiple_draw_begin`/`multiple_draw_end` hooks of `jit.gl.multiple` are left out, so
//!   every draw checks whether to recalculate;
//! - bounds come from the ob3d's `calcbounds` after a recalculation, not from the mesh;
//! - the C's filter on `color`, which recolours the mesh at once, is left out: the colour is read
//!   from the ob3d on every draw;
//! - `dim` has accessors written here (median has no array attributes); a one-value `dim` sets
//!   the first dimension only. `gridmode` is an int (median has no char attributes), no
//!   attribute is basic (`shape` and `dim` are in the C), `rebuild` is not marked undocumented,
//!   and the accessors run on the caller's thread.
//!
//! The tests check the geometry directly and draw through the mock, which cannot show what the
//! chunk holds and refuses `getcolor` with a console error; `calcbounds` is untested.

use std::{
    f64::consts::PI,
    ffi::{c_long, c_void},
    ptr::null_mut,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use median::{
    attr::{AttrBuilder, AttrType},
    error::MaxError,
    jit::{
        DrawContext, JitBuilder, JitObjRef, JitResult, Ob3d,
        ob3d::{GLChunk, GLGridPrim},
        wrapper::{Core, GlKind},
    },
    max_sys::{self, t_jit_error_code as err, t_max_err},
    prelude::*,
    wrapper::WrapperWrapped,
};

const PLANES: usize = 12;

#[derive(JitGlObj)]
#[max(name = "jit.gl.median.gridshape")]
pub struct Gridshape {
    obj: JitObjRef,
    grid: Mutex<Grid>,
    recalc: AtomicBool,
    shape: Mutex<SymbolRef>,
    rad_minor: Float32,
    displaylist: Int64,
    cache_mode: Mutex<SymbolRef>,
    gridmode: Int64,
}

impl Ob3d for Gridshape {
    fn new(b: &mut JitBuilder<Self>) -> Result<Self, NewError> {
        Ok(Self {
            obj: b.obj(),
            grid: Mutex::new(Grid::new([20, 20])),
            recalc: AtomicBool::new(true),
            shape: Mutex::new(sym!("sphere")),
            rad_minor: Float32::new(0.5),
            displaylist: Int64::new(0),
            cache_mode: Mutex::new(sym!("vertexarray")),
            gridmode: Int64::new(0),
        })
    }

    fn class_setup(class: &mut Class<Core<Self, GlKind>>) -> MaxResult<()> {
        // GAP: CLASS_ATTR_SYM_VARSIZE (array attributes)
        let mut b = AttrBuilder::new_accessors("dim", AttrType::Int64, get_dim, set_dim);
        b.label(c"Dim");
        class.add_attribute(b.build().map_err(|_| MaxError::Generic)?)?;
        class.add_sel(c"rebuild", |s: &Self| s.recalculate(), 0)?;
        class.add_sel(c"rebuild_geometry", |s: &Self| s.rebuild(), 0)
    }

    fn draw(&self, context: &DrawContext) -> JitResult<()> {
        let color = self.color();
        let mut grid = self.grid.lock().unwrap();
        let recalculated = self.recalc.swap(false, Ordering::AcqRel);
        if recalculated {
            grid.calc(
                *self.shape.lock().unwrap(),
                self.rad_minor.get().into(),
                color,
            );
        }
        grid.color_surface(color);
        let prim = if self.gridmode.get() != 0 {
            GLGridPrim::Triangle
        } else {
            GLGridPrim::Quad
        };
        let mut chunk = GLChunk::new_grid(context, prim, PLANES, grid.dim[0], grid.dim[1])?;
        {
            let mut vertices = chunk.vertices()?;
            let mut lock = vertices.lock();
            let mut data = lock.data()?;
            let mut source = grid.cells.chunks_exact(PLANES);
            // SAFETY: the chunk's matrix was made in this call and no one else has seen it.
            for mut rows in unsafe { data.chunks_mut::<f32>()? } {
                for cell in rows.entry_iter_mut() {
                    cell.copy_from_slice(source.next().ok_or(err::JIT_ERR_OUT_OF_BOUNDS)?);
                }
            }
        }
        // SAFETY: every vertex was written above, and the chunk is ours until it drops.
        unsafe { chunk.draw()? };
        // GAP: ob3d attributes and methods
        #[cfg(not(test))]
        // SAFETY: the live Jitter object and its chunk, during its draw.
        unsafe {
            let x = self.obj.as_ptr().cast();
            if recalculated && max_sys::jit_attr_getlong(x, sym!("boundcalc").as_ptr()) != 0 {
                max_sys::jit_object_method(
                    x,
                    sym!("calcbounds").as_ptr(),
                    chunk.as_ptr(),
                    null_mut::<c_void>(),
                );
            }
        }
        Ok(())
    }

    fn dest_changed(&self, _: &DrawContext) -> JitResult<()> {
        if self.displaylist.get() != 0 {
            self.rebuild();
        }
        Ok(())
    }
}

#[median::methods]
impl Gridshape {
    #[attr(
        label = "Shape",
        enum = "sphere torus cylinder opencylinder cube opencube plane circle cone capsule"
    )]
    pub fn shape(&self) -> SymbolRef {
        *self.shape.lock().unwrap()
    }
    pub fn set_shape(&self, v: SymbolRef) {
        *self.shape.lock().unwrap() = v;
        self.rebuild();
    }

    #[attr(label = "Radius Minor")]
    pub fn rad_minor(&self) -> f32 {
        self.rad_minor.get()
    }
    pub fn set_rad_minor(&self, v: f32) {
        self.rad_minor.set(v);
        if self.shape() == sym!("torus") {
            self.rebuild();
        }
    }

    #[attr(label = "Displaylist", style = "onoff")]
    pub fn displaylist(&self) -> t_atom_long {
        self.displaylist.get()
    }
    pub fn set_displaylist(&self, v: t_atom_long) {
        self.displaylist.set(v);
        self.rebuild();
    }

    #[attr(label = "Cache Mode", enum = "immediate vertexarray vertexbuffer")]
    pub fn cache_mode(&self) -> SymbolRef {
        *self.cache_mode.lock().unwrap()
    }
    pub fn set_cache_mode(&self, v: SymbolRef) {
        let mut mode = self.cache_mode.lock().unwrap();
        if *mode != v {
            *mode = v;
            self.rebuild();
        }
    }

    // GAP: CLASS_ATTR_CHAR
    #[attr(label = "Grid Mode", style = "onoff")]
    pub fn gridmode(&self) -> t_atom_long {
        self.gridmode.get()
    }
    pub fn set_gridmode(&self, v: t_atom_long) {
        self.gridmode.set(v);
    }
}

impl Gridshape {
    fn rebuild(&self) {
        self.recalc.store(true, Ordering::Release);
    }

    fn recalculate(&self) {
        let color = self.color();
        let mut grid = self.grid.lock().unwrap();
        grid.calc(self.shape(), self.rad_minor.get().into(), color);
        self.recalc.store(false, Ordering::Release);
    }

    /// The ob3d's colour, white if it cannot be read.
    fn color(&self) -> [f32; 4] {
        let mut color = [1.; 4];
        let mut ac: c_long = 0;
        let mut av: *mut max_sys::t_atom = null_mut();
        // GAP: ob3d attributes and methods
        // SAFETY: the live Jitter object answers `getcolor` like an attribute getter, with an
        // atom array it allocates with `jit_getbytes`, freed here.
        unsafe {
            max_sys::jit_object_method(
                self.obj.as_ptr().cast(),
                sym!("getcolor").as_ptr(),
                &mut ac,
                &mut av,
            );
            if !av.is_null() {
                let atoms = std::slice::from_raw_parts(av.cast::<Atom>(), ac.max(0) as usize);
                if atoms.len() == 4 {
                    for (c, a) in color.iter_mut().zip(atoms) {
                        *c = a.get_float() as f32;
                    }
                }
                max_sys::jit_freebytes(av.cast(), ac * size_of::<max_sys::t_atom>() as c_long);
            }
        }
        color
    }
}

/// The vertices of a `dim[0]` by `dim[1]` grid, `PLANES` floats each, dimension 0 fastest:
/// position, texture coordinate, normal and colour.
struct Grid {
    dim: [usize; 2],
    cells: Vec<f32>,
}

/// One vertex.
fn vertex(p: [f64; 3], t: [f64; 2], n: [f64; 3], [r, g, b, a]: [f32; 4]) -> [f32; PLANES] {
    let f = |v: f64| v as f32;
    [
        f(p[0]),
        f(p[1]),
        f(p[2]),
        f(t[0]),
        f(t[1]),
        f(n[0]),
        f(n[1]),
        f(n[2]),
        r,
        g,
        b,
        a,
    ]
}

/// Where column `j` of `numminor` falls around a box: its side (0 to 3, `+x`, `+y`, `-x`, `-y`)
/// and how far along it, from 0 to 1. The C divides in single precision, and with fewer than
/// eight columns some sides have one column and divide 0 by 0.
fn box_side(j: isize, numminor: isize) -> (usize, f64) {
    let tface0 = numminor / 4;
    let tface1 = numminor / 2;
    let tface2 = (3 * numminor) / 4;
    let along = |j: isize, start: isize, end: isize| (j - start) as f32 / (end - start - 1) as f32;
    let (side, t) = if j < tface0 {
        (0, along(j, 0, tface0))
    } else if j < tface1 {
        (1, along(j, tface0, tface1))
    } else if j < tface2 {
        (2, along(j, tface1, tface2))
    } else {
        (3, along(j, tface2, numminor))
    };
    (side, t.into())
}

/// The rim position of a box side at `t` along it, and its outward normal.
fn box_rim(side: usize, t: f64) -> ([f64; 2], [f64; 2]) {
    match side {
        0 => ([1., 2. * t - 1.], [1., 0.]),
        1 => ([1. - 2. * t, 1.], [0., 1.]),
        2 => ([-1., 1. - 2. * t], [-1., 0.]),
        _ => ([2. * t - 1., -1.], [0., -1.]),
    }
}

/// The texture coordinate of a cap's rim at side `side`, `t` along it.
fn cap_tex(side: usize, t: f64) -> [f64; 2] {
    match side {
        0 => [1., 1. - t],
        1 => [1. - t, 0.],
        2 => [0., t],
        _ => [t, 1.],
    }
}

impl Grid {
    fn new(dim: [usize; 2]) -> Self {
        Self {
            dim,
            cells: vec![0.; dim[0] * dim[1] * PLANES],
        }
    }

    fn resize(&mut self, dim: [usize; 2]) {
        self.dim = dim;
        self.cells.resize(dim[0] * dim[1] * PLANES, 0.);
    }

    fn set(&mut self, row: isize, column: isize, v: [f32; PLANES]) {
        let (row, column) = (row as usize, column as usize);
        if row < self.dim[1] && column < self.dim[0] {
            let at = (row * self.dim[0] + column) * PLANES;
            self.cells[at..at + PLANES].copy_from_slice(&v);
        }
    }

    /// The C's `calc_*` for `shape`; anything unknown is a sphere.
    fn calc(&mut self, shape: SymbolRef, rad_minor: f64, color: [f32; 4]) {
        match shape.name().to_bytes() {
            b"plane" => self.plane(color),
            b"cube" => self.cube(color),
            b"opencube" => self.opencube(color),
            b"circle" => self.circle(color),
            b"opencylinder" => self.opencylinder(color),
            b"torus" => self.torus(rad_minor, color),
            b"cylinder" => self.cylinder(color),
            b"capsule" => self.capsule(color),
            b"cone" => self.cone(color),
            _ => self.sphere(color),
        }
    }

    fn majors(&self) -> (isize, isize) {
        (self.dim[1] as isize - 1, self.dim[0] as isize - 1)
    }

    fn sphere(&mut self, color: [f32; 4]) {
        let (nummajor, numminor) = self.majors();
        let (majorstep, minorstep) = (PI / nummajor as f64, 2. * PI / numminor as f64);
        for i in 0..=nummajor {
            let a = i as f64 * majorstep;
            let (r, z) = (a.sin(), a.cos());
            for j in 0..=numminor {
                let c = j as f64 * minorstep;
                let p = [r * c.cos(), r * c.sin(), z];
                let t = [j as f64 / numminor as f64, i as f64 / nummajor as f64];
                self.set(i, j, vertex(p, t, p, color));
            }
        }
    }

    fn opencylinder(&mut self, color: [f32; 4]) {
        let (nummajor, numminor) = self.majors();
        let (majorstep, minorstep) = (2. / nummajor as f64, 2. * PI / numminor as f64);
        for i in 0..=nummajor {
            let z = 1. - i as f64 * majorstep;
            for j in 0..=numminor {
                let c = j as f64 * minorstep;
                let (x, y) = (c.cos(), c.sin());
                let t = [j as f64 / numminor as f64, i as f64 / nummajor as f64];
                self.set(i, j, vertex([x, y, z], t, [x, y, 0.], color));
            }
        }
    }

    fn cylinder(&mut self, color: [f32; 4]) {
        if self.dim[1] < 3 {
            return;
        }
        let nummajor = self.dim[1] as isize - 3;
        let numminor = self.dim[0] as isize - 1;
        let (majorstep, minorstep) = (2. / nummajor as f64, 2. * PI / numminor as f64);
        for j in 0..=numminor {
            let s = j as f64 / numminor as f64;
            self.set(0, j, vertex([0., 0., 1.], [s, 0.], [0., 0., 1.], color));
            self.set(
                nummajor + 2,
                j,
                vertex([0., 0., -1.], [s, 1.], [0., 0., -1.], color),
            );
        }
        for i in 0..=nummajor {
            let z = 1. - i as f64 * majorstep;
            for j in 0..=numminor {
                let c = j as f64 * minorstep;
                let (x, y) = (c.cos(), c.sin());
                let t = [j as f64 / numminor as f64, i as f64 / nummajor as f64];
                self.set(i + 1, j, vertex([x, y, z], t, [x, y, 0.], color));
            }
        }
    }

    fn cube(&mut self, color: [f32; 4]) {
        if self.dim[1] < 3 {
            return;
        }
        let nummajor = self.dim[1] as isize - 5;
        let numminor = self.dim[0] as isize;
        let majorstep = 2. / nummajor as f64;
        for j in 0..numminor {
            self.set(0, j, vertex([0., 0., 1.], [0.5, 0.5], [0., 0., 1.], color));
            let (side, t) = box_side(j, numminor);
            let ([vx, vy], _) = box_rim(side, t);
            let tex = cap_tex(side, t);
            self.set(1, j, vertex([vx, vy, 1.], tex, [0., 0., 1.], color));
        }
        self.box_body(2, nummajor, majorstep, color);
        for j in 0..numminor {
            let (side, t) = box_side(j, numminor);
            let ([vx, vy], _) = box_rim(side, t);
            let tex = cap_tex(side, t);
            self.set(
                nummajor + 3,
                j,
                vertex([vx, vy, -1.], tex, [0., 0., -1.], color),
            );
        }
        for j in 0..numminor {
            let v = vertex([0., 0., -1.], [0.5, 0.5], [0., 0., -1.], color);
            self.set(nummajor + 4, j, v);
        }
    }

    fn opencube(&mut self, color: [f32; 4]) {
        if self.dim[1] < 3 {
            return;
        }
        let nummajor = self.dim[1] as isize - 1;
        self.box_body(0, nummajor, 2. / nummajor as f64, color);
    }

    /// Rows `first` to `first + nummajor` of a box's sides, from `z` 1 down to -1.
    fn box_body(&mut self, first: isize, nummajor: isize, majorstep: f64, color: [f32; 4]) {
        let numminor = self.dim[0] as isize;
        for i in 0..=nummajor {
            let vz = 1. - i as f64 * majorstep;
            for j in 0..numminor {
                let (side, t) = box_side(j, numminor);
                let ([vx, vy], [nx, ny]) = box_rim(side, t);
                let tex = [t, i as f64 / nummajor as f64];
                self.set(first + i, j, vertex([vx, vy, vz], tex, [nx, ny, 0.], color));
            }
        }
    }

    fn circle(&mut self, color: [f32; 4]) {
        let (nummajor, numminor) = self.majors();
        let (majorstep, minorstep) = (1. / nummajor as f64, 2. * PI / numminor as f64);
        for i in 0..=nummajor {
            let r = 1. - i as f64 * majorstep;
            for j in 0..=numminor {
                let c = 2. * PI - j as f64 * minorstep;
                let p = [r * c.cos(), r * c.sin(), 0.];
                let t = [j as f64 / numminor as f64, i as f64 / nummajor as f64];
                self.set(i, j, vertex(p, t, [0., 0., 1.], color));
            }
        }
    }

    fn plane(&mut self, color: [f32; 4]) {
        let (nummajor, numminor) = self.majors();
        let (majorstep, minorstep) = (2. / nummajor as f64, 2. / numminor as f64);
        for i in 0..=nummajor {
            let y = i as f64 * majorstep - 1.;
            for j in 0..=numminor {
                let x = 1. - j as f64 * minorstep;
                let t = [1. - j as f64 / numminor as f64, i as f64 / nummajor as f64];
                self.set(i, j, vertex([x, y, 0.], t, [0., 0., 1.], color));
            }
        }
    }

    fn torus(&mut self, radminor: f64, color: [f32; 4]) {
        let (nummajor, numminor) = self.majors();
        let (majorstep, minorstep) = (2. * PI / nummajor as f64, 2. * PI / numminor as f64);
        for i in 0..=nummajor {
            let a = i as f64 * majorstep;
            let (x, y) = (a.cos(), a.sin());
            for j in 0..=numminor {
                let b = j as f64 * minorstep;
                let c = b.cos();
                let r = radminor * c + 1.;
                let z = b.sin();
                let t = [j as f64 / numminor as f64, i as f64 / nummajor as f64];
                let v = vertex([x * r, y * r, z * radminor], t, [x * c, y * c, z], color);
                self.set(i, j, v);
            }
        }
    }

    fn capsule(&mut self, color: [f32; 4]) {
        let (nummajor, numminor) = self.majors();
        let (majorstep, minorstep) = (PI / nummajor as f64, 2. * PI / numminor as f64);
        for i in 0..=nummajor {
            let a = i as f64 * majorstep;
            let (r, znorm) = (a.sin(), a.cos());
            let z = if znorm > 0. { znorm + 0.5 } else { znorm - 0.5 };
            for j in 0..=numminor {
                let c = j as f64 * minorstep;
                let (x, y) = (r * c.cos(), r * c.sin());
                let t = [j as f64 / numminor as f64, i as f64 / nummajor as f64];
                self.set(i, j, vertex([x, y, z], t, [x, y, znorm], color));
            }
        }
    }

    fn cone(&mut self, color: [f32; 4]) {
        if self.dim[1] < 3 {
            return;
        }
        let nummajor = self.dim[1] as isize - 2;
        let numminor = self.dim[0] as isize - 1;
        let (majorstep, minorstep) = (2. / nummajor as f64, 2. * PI / numminor as f64);
        for i in 0..=nummajor {
            let z = 0.5 - i as f64 * majorstep * 0.5;
            let scaler = i as f64 * majorstep * 0.5;
            for j in 0..=numminor {
                let c = j as f64 * minorstep;
                let (x, y) = (c.cos() * scaler, c.sin() * scaler);
                let t = [j as f64 / numminor as f64, i as f64 / nummajor as f64];
                self.set(i, j, vertex([x, y, z], t, [x, y, 0.], color));
            }
        }
        for j in 0..=numminor {
            let t = [j as f64 / numminor as f64, 0.];
            let v = vertex([0., 0., -0.5], t, [0., 0., 1.], color);
            self.set(nummajor + 1, j, v);
        }
    }

    /// Set every vertex's colour, unless the first already has it.
    fn color_surface(&mut self, color: [f32; 4]) {
        if self.cells.get(8..12) == Some(&color[..]) {
            return;
        }
        for cell in self.cells.chunks_exact_mut(PLANES) {
            cell[8..12].copy_from_slice(&color);
        }
    }
}

/// The `dim` getter.
///
/// # Safety
/// Only Jitter calls it, as an attribute getter, with the `ac` and `av` it passes getters.
unsafe extern "C" fn get_dim(
    w: &Core<Gridshape, GlKind>,
    _attr: *mut c_void,
    ac: *mut c_long,
    av: *mut *mut max_sys::t_atom,
) -> t_max_err {
    median::ffi::guard_or(w, "dim getter", err::JIT_ERR_GENERIC as _, || {
        let dim = w.wrapped().grid.lock().unwrap().dim;
        // SAFETY: the getter's own `ac` and `av`
        unsafe { write_atoms(ac, av, dim.map(|d| Atom::from(d as t_atom_long))) }
    })
}

/// The `dim` setter: each dimension at least 2, as the C's clip filter makes it.
///
/// # Safety
/// Only Jitter calls it, as an attribute setter, with `av` holding `ac` atoms.
unsafe extern "C" fn set_dim(
    w: &Core<Gridshape, GlKind>,
    _attr: *mut c_void,
    ac: c_long,
    av: *mut max_sys::t_atom,
) -> t_max_err {
    median::ffi::guard_or(w, "dim setter", err::JIT_ERR_GENERIC as _, || {
        if ac <= 0 || av.is_null() {
            return 0;
        }
        // SAFETY: Jitter passes `ac` valid atoms; `Atom` is a transparent `t_atom`.
        let atoms: &[Atom] = unsafe { std::slice::from_raw_parts(av.cast(), ac as usize) };
        let shape = w.wrapped();
        let mut grid = shape.grid.lock().unwrap();
        let mut dim = grid.dim;
        for (d, a) in dim.iter_mut().zip(atoms) {
            *d = a.get_int().max(2) as usize;
        }
        grid.resize(dim);
        shape.rebuild();
        0
    })
}

/// Fill a Jitter getter's results: into the array the caller passes, or a new one it frees.
///
/// # Safety
/// `ac` and `av` are null or the out-parameters Jitter passes to an attribute getter; a non-null
/// `*av` holds `*ac` atoms.
unsafe fn write_atoms<const N: usize>(
    ac: *mut c_long,
    av: *mut *mut max_sys::t_atom,
    atoms: [Atom; N],
) -> t_max_err {
    if ac.is_null() || av.is_null() {
        return 0;
    }
    // SAFETY: the caller's contract; `Atom` is a transparent `t_atom`
    unsafe {
        if (*av).is_null() || *ac <= 0 {
            // GAP: Custom attribute accessors
            let p = max_sys::jit_getbytes((N * size_of::<max_sys::t_atom>()) as c_long);
            if p.is_null() {
                *ac = 0;
                return err::JIT_ERR_OUT_OF_MEM as _;
            }
            *av = p.cast();
            *ac = N as c_long;
        } else {
            *ac = (*ac).min(N as c_long);
        }
        for (i, atom) in atoms.into_iter().take(*ac as usize).enumerate() {
            (*av).add(i).cast::<Atom>().write(atom);
        }
    }
    0
}

median::ext_main! { Gridshape }

#[cfg(test)]
mod tests {
    use super::*;
    use median_mock::Atom;

    const WHITE: [f32; 4] = [1.; 4];

    fn shape(name: &str, dim: [usize; 2]) -> Grid {
        let _k = median_mock::lock();
        let mut g = Grid::new(dim);
        g.calc(SymbolRef::try_from(name).unwrap(), 0.5, WHITE);
        g
    }

    fn cells(g: &Grid) -> impl Iterator<Item = &[f32]> {
        g.cells.chunks_exact(PLANES)
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    #[test]
    fn sphere_vertices_are_unit_normals_from_pole_to_pole() {
        let g = shape("sphere", [9, 5]);
        for c in cells(&g) {
            let r = (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt();
            assert!(close(r, 1.), "{c:?}");
            assert_eq!(c[..3], c[5..8]);
        }
        assert!(close(g.cells[2], 1.));
        assert!(close(g.cells[g.cells.len() - PLANES + 2], -1.));
        assert_eq!(&g.cells[8..12], WHITE);
    }

    #[test]
    fn an_unknown_shape_is_a_sphere() {
        assert_eq!(shape("blob", [6, 6]).cells, shape("sphere", [6, 6]).cells);
    }

    #[test]
    fn plane_corners_and_texture() {
        let g = shape("plane", [3, 2]);
        let c: Vec<&[f32]> = cells(&g).collect();
        assert_eq!(c[0][..5], [1., -1., 0., 1., 0.]);
        assert_eq!(c[2][..5], [-1., -1., 0., 0., 0.]);
        assert_eq!(c[5][..5], [-1., 1., 0., 0., 1.]);
    }

    #[test]
    fn torus_tube_has_the_minor_radius() {
        let g = shape("torus", [8, 8]);
        for c in cells(&g) {
            let ring = (c[0] * c[0] + c[1] * c[1]).sqrt() - 1.;
            assert!(close((ring * ring + c[2] * c[2]).sqrt(), 0.5), "{c:?}");
        }
    }

    #[test]
    fn cylinder_has_caps_on_its_first_and_last_rows() {
        let g = shape("cylinder", [5, 5]);
        let c: Vec<&[f32]> = cells(&g).collect();
        assert!(c[..5].iter().all(|v| v[..3] == [0., 0., 1.]));
        assert!(c[20..].iter().all(|v| v[..3] == [0., 0., -1.]));
        assert!(c[5..20].iter().all(|v| close(v[0].hypot(v[1]), 1.)));
    }

    #[test]
    fn cube_and_opencube_sides_lie_on_the_box() {
        let g = shape("cube", [8, 7]);
        let c: Vec<&[f32]> = cells(&g).collect();
        assert!(c[..8].iter().all(|v| v[..5] == [0., 0., 1., 0.5, 0.5]));
        assert!(c[48..].iter().all(|v| v[..5] == [0., 0., -1., 0.5, 0.5]));
        for v in &c[8..48] {
            assert!(v[0].abs() == 1. || v[1].abs() == 1., "{v:?}");
        }
        // Side 0 (+x) runs y from -1 to 1 in two columns.
        assert_eq!(c[16][..3], [1., -1., 1.]);
        assert_eq!(c[17][..3], [1., 1., 1.]);
        let g = shape("opencube", [8, 3]);
        assert!(cells(&g).all(|v| v[0].abs() == 1. || v[1].abs() == 1.));
        assert_eq!(g.cells[2], 1.);
        assert_eq!(g.cells[g.cells.len() - PLANES + 2], -1.);
    }

    #[test]
    fn short_grids_leave_cylinder_cube_and_cone_alone() {
        for name in ["cylinder", "cube", "opencube", "cone"] {
            assert!(shape(name, [4, 2]).cells.iter().all(|&v| v == 0.), "{name}");
        }
    }

    #[test]
    fn circle_cone_capsule_and_opencylinder() {
        let g = shape("circle", [5, 3]);
        assert!(cells(&g).take(5).all(|v| close(v[0].hypot(v[1]), 1.)));
        assert!(
            cells(&g)
                .skip(10)
                .all(|v| close(v[0], 0.) && close(v[1], 0.))
        );
        let g = shape("cone", [5, 4]);
        assert!(cells(&g).take(5).all(|v| v[..3] == [0., 0., 0.5]));
        assert!(
            cells(&g)
                .skip(10)
                .take(5)
                .all(|v| close(v[0].hypot(v[1]), 1.))
        );
        assert!(cells(&g).skip(15).all(|v| v[..3] == [0., 0., -0.5]));
        let g = shape("capsule", [5, 3]);
        assert!(close(g.cells[2], 1.5));
        assert!(close(g.cells[g.cells.len() - PLANES + 2], -1.5));
        let g = shape("opencylinder", [5, 3]);
        assert!(cells(&g).all(|v| close(v[0].hypot(v[1]), 1.) && v[7] == 0.));
    }

    type Draw = unsafe extern "C" fn(*mut c_void) -> max_sys::t_jit_err;

    fn draw(k: &median_mock::Kernel, core: *mut c_void) -> max_sys::t_jit_err {
        let (f, _) = k.method(core, "ob3d_draw").unwrap();
        // SAFETY: `ob3d_draw` is registered with this signature, and `core` is live.
        unsafe { std::mem::transmute::<usize, Draw>(f)(core) }
    }

    #[test]
    fn attributes_and_drawing() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object(
            "jit.gl.median.gridshape",
            &["ctx".into(), "@shape".into(), "torus".into()],
        );
        assert!(!x.is_null(), "{:?}", k.console_log());
        let core = median_mock::jit::object_for(x);
        assert_eq!(k.attr_get(x, "shape").unwrap(), [Atom::from("torus")]);
        assert_eq!(k.attr_get(x, "dim").unwrap(), [20i64.into(), 20i64.into()]);
        assert_eq!(k.attr_get(x, "rad_minor").unwrap(), [0.5.into()]);
        assert_eq!(
            k.attr_get(x, "cache_mode").unwrap(),
            [Atom::from("vertexarray")]
        );
        assert_eq!(draw(&k, core), 0);
        assert_eq!(median_mock::jit::live_chunk_count(), 0);
        k.attr_set(x, "dim", &[4i64.into(), 0i64.into()]).unwrap();
        assert_eq!(k.attr_get(x, "dim").unwrap(), [4i64.into(), 2i64.into()]);
        k.attr_set(x, "gridmode", &[1i64.into()]).unwrap();
        assert_eq!(draw(&k, core), 0);
        k.send(core, "rebuild", &[]).unwrap();
        k.send(core, "rebuild_geometry", &[]).unwrap();
        assert_eq!(draw(&k, core), 0);
        assert!(k.free_object(x));
        assert_eq!(median_mock::jit::context_count(), 0);
    }
}
