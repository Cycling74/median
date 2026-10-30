//! A port of the SDK's `gl/jit.gl.videoplane`: draws a grid-subdivided plane textured with the
//! matrices it receives, through an internal `jit.gl.texture`.
//!
//! Deviations from the C:
//! - the grid is drawn with `jit_ob3d_draw_chunk` in both modes, with `gridmode` choosing a
//!   `quad_grid` or `tri_grid` chunk. The C draws it itself with `glBegin`/`glVertex3f` unless
//!   `matrixoutput` is on, and median has no raw OpenGL, so `displaylist` (a GL display list) is
//!   kept as an attribute that only triggers a recalculation;
//! - the geometry is kept in a `Vec` and copied into a chunk made on every draw, since median's
//!   `GLChunk` lives only inside a GL callback;
//! - `dim` has accessors written here (median has no array attributes); a one-value `dim` sets
//!   the first dimension only. `gridmode` is an int (median has no char attributes);
//! - `jit_matrix` checks for the texture before looking up the matrix. The C reads the matrix's
//!   info and the texture's `dim` into values it never uses, from an uninitialized pointer when
//!   the name is empty; the port leaves that out;
//! - the C sets the ob3d's `color` to `1 1 1 1` in `new`, its default, and the port does not;
//! - `sendtexture` is a deferred method of the Max object, not of the Jitter object, and
//!   `jit_matrix` is not `A_USURP_LOW`; the attribute accessors run on the caller's thread.
//!
//! The mock has no `jit.gl.texture` class, so the tests cover an object whose texture could not
//! be made: everything that talks to the texture, the `drawto` filter, `calcbounds` and the
//! colour read from the ob3d (`getcolor`, which the mock refuses with a console error) are
//! untested.

use std::{
    ffi::{c_long, c_void},
    ptr::null_mut,
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicPtr, Ordering},
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
#[max(name = "jit.gl.median.videoplane")]
pub struct Videoplane {
    obj: JitObjRef,
    /// The internal `jit_gl_texture`, null if it could not be made; owned, freed in `Drop`.
    texture: AtomicPtr<c_void>,
    grid: Mutex<Grid>,
    recalc: AtomicBool,
    displaylist: Int64,
    gridmode: Int64,
    interp: Int64,
    nudge: Float32,
    tex_offset_x: Float32,
    tex_offset_y: Float32,
    tex_scale_x: Float32,
    tex_scale_y: Float32,
    rect_tex: Int64,
    client_storage: Int64,
    colormode: Mutex<SymbolRef>,
    texturename: Mutex<SymbolRef>,
}

impl Ob3d for Videoplane {
    fn new(b: &mut JitBuilder<Self>) -> Result<Self, NewError> {
        let obj = b.obj();
        let rect_tex = 1;
        let client_storage = cfg!(target_os = "macos") as t_atom_long;
        // GAP: Internal Jitter GL objects
        // SAFETY: a Jitter class name and the destination, as `jit_object_new` takes them.
        let texture =
            unsafe { max_sys::jit_object_new(sym!("jit_gl_texture").as_ptr(), b.destination()) };
        let texturename = if texture.is_null() {
            obj.error("jit.gl.videoplane: could not create texture");
            SymbolRef::default()
        } else {
            // SAFETY: `jit_symbol_unique` takes nothing and returns an interned symbol.
            let name = unsafe { SymbolRef::from_raw(max_sys::jit_symbol_unique()) };
            let mode = if client_storage != 0 {
                sym!("dynamic")
            } else {
                sym!("static")
            };
            // SAFETY: the texture was made above; the ob3d of `obj` exists before `new` runs.
            unsafe {
                max_sys::jit_attr_setsym(texture, sym!("name").as_ptr(), name.as_ptr());
                max_sys::jit_attr_setsym(
                    texture,
                    sym!("defaultimage").as_ptr(),
                    sym!("black").as_ptr(),
                );
                max_sys::jit_attr_setlong(texture, sym!("rectangle").as_ptr(), rect_tex);
                max_sys::jit_attr_setsym(texture, sym!("mode").as_ptr(), mode.as_ptr());
                max_sys::jit_attr_setsym(
                    obj.as_ptr().cast(),
                    sym!("texture").as_ptr(),
                    name.as_ptr(),
                );
            }
            name
        };
        Ok(Self {
            obj,
            texture: AtomicPtr::new(texture),
            grid: Mutex::new(Grid::new([20, 20])),
            recalc: AtomicBool::new(true),
            displaylist: Int64::new(0),
            gridmode: Int64::new(0),
            interp: Int64::new(1),
            // A hack for the edges, in the C.
            nudge: Float32::new(0.0001),
            tex_offset_x: Float32::new(0.),
            tex_offset_y: Float32::new(0.),
            tex_scale_x: Float32::new(1.),
            tex_scale_y: Float32::new(1.),
            rect_tex: Int64::new(rect_tex),
            client_storage: Int64::new(client_storage),
            colormode: Mutex::new(sym!("argb")),
            texturename: Mutex::new(texturename),
        })
    }

    fn class_setup(class: &mut Class<Core<Self, GlKind>>) -> MaxResult<()> {
        // GAP: CLASS_ATTR_SYM_VARSIZE (array attributes)
        let b = AttrBuilder::new_accessors("dim", AttrType::Int64, get_dim, set_dim);
        class.add_attribute(b.build().map_err(|_| MaxError::Generic)?)?;
        class.add_gimme(c"jit_matrix", Self::jit_matrix)?;
        class.add_sel(c"rebuild_geometry", |s: &Self| s.rebuild(), 0)?;
        // GAP: Attribute filters
        // SAFETY: the class is live, and `drawto_filter` has the signature of a set filter.
        unsafe {
            let drawto = max_sys::jit_class_attr_get(class.inner().cast(), sym!("drawto").as_ptr());
            if !drawto.is_null() {
                let f = std::mem::transmute::<
                    unsafe extern "C" fn(
                        &Core<Videoplane, GlKind>,
                        *mut c_void,
                        c_long,
                        *mut max_sys::t_atom,
                    ) -> t_max_err,
                    unsafe extern "C" fn(*mut c_void) -> *mut c_void,
                >(drawto_filter);
                max_sys::jit_attr_addfilterset_proc(drawto, Some(f));
            }
        }
        Ok(())
    }

    fn draw(&self, context: &DrawContext) -> JitResult<()> {
        let nudge = self.nudge.get().clamp(0., 0.5);
        self.nudge.set(nudge);
        let color = self.color();
        let mut grid = self.grid.lock().unwrap();
        if self.recalc.swap(false, Ordering::AcqRel) {
            grid.calc_plane(&self.texcoords(nudge), color);
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
            if max_sys::jit_attr_getlong(x, sym!("boundcalc").as_ptr()) != 0
                && max_sys::jit_attr_getlong(x, sym!("matrixoutput").as_ptr()) == 0
            {
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

impl Drop for Videoplane {
    fn drop(&mut self) {
        let texture = *self.texture.get_mut();
        if !texture.is_null() {
            // SAFETY: the texture `new` made, freed once.
            unsafe { max_sys::jit_object_free(texture) };
        }
    }
}

#[median::methods]
impl Videoplane {
    #[attr(label = "Displaylist", style = "onoff")]
    pub fn displaylist(&self) -> t_atom_long {
        self.displaylist.get()
    }
    pub fn set_displaylist(&self, v: t_atom_long) {
        self.displaylist.set(v);
        self.rebuild();
    }

    // GAP: CLASS_ATTR_CHAR
    #[attr(label = "Grid Mode", enum = "trigrid quadgrid")]
    pub fn gridmode(&self) -> t_atom_long {
        self.gridmode.get()
    }
    pub fn set_gridmode(&self, v: t_atom_long) {
        self.gridmode.set(v);
        self.rebuild();
    }

    #[attr(label = "Interp", style = "onoff")]
    pub fn interp(&self) -> t_atom_long {
        self.interp.get()
    }
    pub fn set_interp(&self, v: t_atom_long) {
        self.interp.set(v);
        let filter = if v != 0 {
            sym!("linear")
        } else {
            sym!("nearest")
        };
        self.texture_sym(sym!("filter"), filter);
    }

    #[attr(label = "Nudge")]
    pub fn nudge(&self) -> f32 {
        self.nudge.get()
    }
    pub fn set_nudge(&self, v: f32) {
        self.nudge.set(v);
        self.rebuild();
    }

    #[attr(label = "Tex Offset X")]
    pub fn tex_offset_x(&self) -> f32 {
        self.tex_offset_x.get()
    }
    pub fn set_tex_offset_x(&self, v: f32) {
        self.tex_offset_x.set(v);
        self.rebuild();
    }

    #[attr(label = "Tex Offset Y")]
    pub fn tex_offset_y(&self) -> f32 {
        self.tex_offset_y.get()
    }
    pub fn set_tex_offset_y(&self, v: f32) {
        self.tex_offset_y.set(v);
        self.rebuild();
    }

    #[attr(label = "Tex Scale X")]
    pub fn tex_scale_x(&self) -> f32 {
        self.tex_scale_x.get()
    }
    pub fn set_tex_scale_x(&self, v: f32) {
        self.tex_scale_x.set(v);
        self.rebuild();
    }

    #[attr(label = "Tex Scale Y")]
    pub fn tex_scale_y(&self) -> f32 {
        self.tex_scale_y.get()
    }
    pub fn set_tex_scale_y(&self, v: f32) {
        self.tex_scale_y.set(v);
        self.rebuild();
    }

    #[attr(label = "Rect Tex")]
    pub fn rect_tex(&self) -> t_atom_long {
        self.rect_tex.get()
    }
    pub fn set_rect_tex(&self, v: t_atom_long) {
        self.rect_tex.set(v);
        if let Some(texture) = self.texture() {
            // SAFETY: the live texture `new` made.
            unsafe { max_sys::jit_attr_setlong(texture, sym!("rectangle").as_ptr(), v) };
        }
    }

    #[attr(label = "Client Storage")]
    pub fn client_storage(&self) -> t_atom_long {
        self.client_storage.get()
    }
    pub fn set_client_storage(&self, v: t_atom_long) {
        self.client_storage.set(v);
        let mode = if v != 0 {
            sym!("dynamic")
        } else {
            sym!("static")
        };
        self.texture_sym(sym!("mode"), mode);
    }

    #[attr(label = "Colormode", enum = "argb uyvy")]
    pub fn colormode(&self) -> SymbolRef {
        *self.colormode.lock().unwrap()
    }
    pub fn set_colormode(&self, v: SymbolRef) {
        *self.colormode.lock().unwrap() = v;
        self.texture_sym(sym!("colormode"), v);
    }

    #[attr(label = "Texture Name")]
    pub fn texturename(&self) -> SymbolRef {
        *self.texturename.lock().unwrap()
    }
    pub fn set_texturename(&self, v: SymbolRef) {
        *self.texturename.lock().unwrap() = v;
        self.texture_sym(sym!("name"), v);
        // GAP: ob3d attributes and methods
        // SAFETY: the live Jitter object; `texture` is an ob3d attribute.
        unsafe {
            max_sys::jit_attr_setsym(
                self.obj.as_ptr().cast(),
                sym!("texture").as_ptr(),
                v.as_ptr(),
            )
        };
    }

    /// Forward a message to the texture: `sendtexture <message> <arguments>`.
    #[gimme("sendtexture")]
    #[defer_low]
    pub fn sendtexture(&self, atoms: &[Atom]) {
        let (Some(texture), Some((message, rest))) = (self.texture(), atoms.split_first()) else {
            return;
        };
        let argv = if rest.is_empty() {
            null_mut()
        } else {
            rest.as_ptr().cast_mut().cast()
        };
        // GAP: Internal Jitter GL objects
        // SAFETY: the live texture and `rest.len()` atoms, which the callee only reads.
        unsafe {
            max_sys::object_method_typed(
                texture,
                message.get_symbol().as_ptr(),
                rest.len() as c_long,
                argv,
                null_mut(),
            );
        }
    }
}

impl Videoplane {
    fn texture(&self) -> Option<*mut c_void> {
        let p = self.texture.load(Ordering::Acquire);
        (!p.is_null()).then_some(p)
    }

    fn texture_sym(&self, attr: SymbolRef, value: SymbolRef) {
        if let Some(texture) = self.texture() {
            // SAFETY: the live texture `new` made.
            unsafe { max_sys::jit_attr_setsym(texture, attr.as_ptr(), value.as_ptr()) };
        }
    }

    fn rebuild(&self) {
        self.recalc.store(true, Ordering::Release);
    }

    /// Hand a matrix to the texture, then make the ob3d draw with it again.
    fn jit_matrix(&self, sel: &SymbolRef, atoms: &[Atom]) {
        let Some(texture) = self.texture() else {
            return;
        };
        let name = atoms.first().map(Atom::get_symbol).unwrap_or_default();
        // GAP: Owned and named matrices
        // SAFETY: `jit_object_findregistered` only looks the name up.
        if !name.is_empty()
            && unsafe { max_sys::jit_object_findregistered(name.as_ptr()) }.is_null()
        {
            self.obj
                .error("jit.gl.videoplane: couldn't get matrix object!");
            return;
        }
        let name = *self.texturename.lock().unwrap();
        // SAFETY: the live texture, sent its `jit_matrix` method's arguments; the atoms are only
        // read.
        unsafe {
            max_sys::jit_object_method(
                texture,
                sel.as_ptr(),
                sel.as_ptr(),
                atoms.len() as c_long,
                atoms.as_ptr().cast_mut().cast::<max_sys::t_atom>(),
            );
            max_sys::jit_attr_setsym(
                self.obj.as_ptr().cast(),
                sym!("texture").as_ptr(),
                name.as_ptr(),
            );
        }
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

    fn texcoords(&self, nudge: f32) -> TexCoords {
        TexCoords {
            nudge: nudge.into(),
            offset: [
                self.tex_offset_x.get().into(),
                self.tex_offset_y.get().into(),
            ],
            scale: [self.tex_scale_x.get().into(), self.tex_scale_y.get().into()],
        }
    }
}

struct TexCoords {
    nudge: f64,
    offset: [f64; 2],
    scale: [f64; 2],
}

/// The vertices of a `dim[0]` by `dim[1]` grid, `PLANES` floats each, dimension 0 fastest:
/// position, texture coordinate, normal and colour.
struct Grid {
    dim: [usize; 2],
    cells: Vec<f32>,
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

    fn calc_plane(&mut self, t: &TexCoords, [r, g, b, a]: [f32; 4]) {
        let nummajor = self.dim[1] as f64 - 1.;
        let numminor = self.dim[0] as f64 - 1.;
        let lo = t.nudge;
        let hi = 1. - lo;
        let tex_nummajor_inv = (hi - lo) / nummajor;
        let tex_numminor_inv = (hi - lo) / numminor;
        let tx = if t.scale[0] == 0. {
            0.
        } else {
            1. / t.scale[0]
        };
        let ty = if t.scale[1] == 0. {
            0.
        } else {
            1. / t.scale[1]
        };
        let width = self.dim[0];
        for (k, cell) in self.cells.chunks_exact_mut(PLANES).enumerate() {
            let (i, j) = ((k / width) as f64, (k % width) as f64);
            let s = (lo + (numminor - j) * tex_numminor_inv) * tx - t.offset[0];
            let tt = 1. - ((lo + (nummajor - i) * tex_nummajor_inv) * ty - t.offset[1]);
            let x = 1. - j * 2. / numminor;
            let y = i * 2. / nummajor - 1.;
            let s = s.max(lo).min(hi);
            let tt = tt.max(lo).min(hi);
            cell.copy_from_slice(&[
                x as f32, y as f32, 0., s as f32, tt as f32, 0., 0., 1., r, g, b, a,
            ]);
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
    w: &Core<Videoplane, GlKind>,
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
    w: &Core<Videoplane, GlKind>,
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
        let plane = w.wrapped();
        let mut grid = plane.grid.lock().unwrap();
        let mut dim = grid.dim;
        for (d, a) in dim.iter_mut().zip(atoms) {
            *d = a.get_int().max(2) as usize;
        }
        grid.resize(dim);
        plane.rebuild();
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

/// A set filter on the ob3d's `drawto`: the texture follows the object's destination.
///
/// # Safety
/// Only Jitter calls it, as a set filter, with `argv` holding `argc` atoms.
unsafe extern "C" fn drawto_filter(
    w: &Core<Videoplane, GlKind>,
    _attr: *mut c_void,
    argc: c_long,
    argv: *mut max_sys::t_atom,
) -> t_max_err {
    median::ffi::guard_or(w, "drawto filter", err::JIT_ERR_GENERIC as _, || {
        if argc > 0 && !argv.is_null() {
            // SAFETY: Jitter passes `argc` valid atoms; `Atom` is a transparent `t_atom`.
            let to = unsafe { (*argv.cast::<Atom>()).get_symbol() };
            w.wrapped().texture_sym(sym!("drawto"), to);
        }
        0
    })
}

median::ext_main! { Videoplane }

#[cfg(test)]
mod tests {
    use super::*;
    use median_mock::Atom;

    type Draw = unsafe extern "C" fn(*mut c_void) -> max_sys::t_jit_err;

    fn setup(k: &median_mock::Kernel) -> (*mut c_void, *mut c_void) {
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object("jit.gl.median.videoplane", &["ctx".into()]);
        assert!(!x.is_null(), "{:?}", k.console_log());
        (x, median_mock::jit::object_for(x))
    }

    fn draw(k: &median_mock::Kernel, core: *mut c_void) -> max_sys::t_jit_err {
        let (f, _) = k.method(core, "ob3d_draw").unwrap();
        // SAFETY: `ob3d_draw` is registered with this signature, and `core` is live.
        unsafe { std::mem::transmute::<usize, Draw>(f)(core) }
    }

    fn plane(dim: [usize; 2], nudge: f64) -> Grid {
        let mut g = Grid::new(dim);
        let t = TexCoords {
            nudge,
            offset: [0., 0.],
            scale: [1., 1.],
        };
        g.calc_plane(&t, [0.5, 0.25, 1., 1.]);
        g
    }

    #[test]
    fn the_plane_spans_minus_one_to_one_facing_z() {
        let g = plane([3, 2], 0.);
        let cell = |i: usize| &g.cells[i * PLANES..(i + 1) * PLANES];
        assert_eq!(
            cell(0),
            [1., -1., 0., 1., 0., 0., 0., 1., 0.5, 0.25, 1., 1.]
        );
        assert_eq!(&cell(1)[..5], [0., -1., 0., 0.5, 0.]);
        assert_eq!(&cell(2)[..5], [-1., -1., 0., 0., 0.]);
        assert_eq!(&cell(5)[..5], [-1., 1., 0., 0., 1.]);
    }

    #[test]
    fn nudge_keeps_texture_coordinates_off_the_edges() {
        let g = plane([2, 2], 0.25);
        for cell in g.cells.chunks_exact(PLANES) {
            assert!((0.25..=0.75).contains(&cell[3]), "{cell:?}");
            assert!((0.25..=0.75).contains(&cell[4]), "{cell:?}");
        }
    }

    #[test]
    fn color_surface_recolours_every_vertex() {
        let mut g = plane([2, 2], 0.);
        g.color_surface([0., 0., 0., 1.]);
        assert!(
            g.cells
                .chunks_exact(PLANES)
                .all(|c| c[8..] == [0., 0., 0., 1.])
        );
    }

    #[test]
    fn without_a_texture_it_says_so_and_still_draws() {
        let k = median_mock::lock();
        let (x, core) = setup(&k);
        assert!(
            k.take_console_log()
                .iter()
                .any(|e| e.text == "jit.gl.videoplane: could not create texture")
        );
        assert_eq!(k.attr_get(x, "texturename").unwrap(), [Atom::from("")]);
        assert_eq!(draw(&k, core), 0);
        assert_eq!(median_mock::jit::live_chunk_count(), 0);
        // jit_matrix and sendtexture do nothing without a texture.
        k.send(core, "jit_matrix", &["m".into()]).unwrap();
        k.send(x, "sendtexture", &["dim".into(), 4i64.into()])
            .unwrap();
        k.run_main_queue();
        assert!(k.take_outlet_log().is_empty());
        assert!(k.free_object(x));
        assert_eq!(median_mock::jit::context_count(), 0);
    }

    #[test]
    fn attributes_have_the_c_defaults_and_dim_is_at_least_two() {
        let k = median_mock::lock();
        let (x, core) = setup(&k);
        assert_eq!(k.attr_get(x, "dim").unwrap(), [20i64.into(), 20i64.into()]);
        assert_eq!(k.attr_get(x, "interp").unwrap(), [1i64.into()]);
        assert_eq!(k.attr_get(x, "nudge").unwrap(), [(0.0001f32 as f64).into()]);
        assert_eq!(k.attr_get(x, "colormode").unwrap(), [Atom::from("argb")]);
        k.attr_set(x, "dim", &[5i64.into(), 1i64.into()]).unwrap();
        assert_eq!(k.attr_get(x, "dim").unwrap(), [5i64.into(), 2i64.into()]);
        k.attr_set(x, "dim", &[3i64.into()]).unwrap();
        assert_eq!(k.attr_get(x, "dim").unwrap(), [3i64.into(), 2i64.into()]);
        k.attr_set(x, "nudge", &[0.9.into()]).unwrap();
        k.attr_set(x, "gridmode", &[1i64.into()]).unwrap();
        assert_eq!(draw(&k, core), 0);
        // draw clamps nudge as the C does.
        assert_eq!(k.attr_get(x, "nudge").unwrap(), [0.5.into()]);
        k.send(core, "rebuild_geometry", &[]).unwrap();
        assert_eq!(draw(&k, core), 0);
        assert!(k.free_object(x));
    }
}
