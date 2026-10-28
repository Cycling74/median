//! Ownership tests use CPU fixtures, not Max's renderer or undocumented geometry uploads.
use core::ffi::c_void;
use median::{
    jit::{DrawContext, JitBuilder, JitResult, Ob3d, ob3d::*},
    prelude::*,
};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

static PANIC: AtomicBool = AtomicBool::new(false);
static COMPLETED: AtomicUsize = AtomicUsize::new(0);

#[derive(JitGlObj)]
#[max(name = "jit.gl.helper.test", no_methods)]
pub struct Helpers;
impl Ob3d for Helpers {
    fn new(_: &mut JitBuilder<Self>) -> Result<Self, NewError> {
        Ok(Self)
    }
    fn draw(&self, context: &DrawContext) -> JitResult<()> {
        assert!(GLChunk::new(context, GLPrim::Triangles, 0, 3, None).is_err());
        assert!(GLChunk::new(context, GLPrim::Triangles, 3, usize::MAX, None).is_err());
        assert!(GLChunk::new_grid(context, GLGridPrim::Quad, 3, 0, 2).is_err());
        let mut chunk = GLChunk::new(context, GLPrim::Triangles, 3, 3, Some(3))?;
        assert_eq!(chunk.primitive()?, GLChunkPrim::Prim(GLPrim::Triangles));
        {
            let mut vertices = chunk.vertices()?;
            let mut guard = vertices.lock();
            assert_eq!(guard.info()?.planes()?, 3);
            assert_eq!(guard.info()?.dimensions()?, &[3]);
            // SAFETY: this test's CPU fixture is zero initialized, exclusive and 36 bytes long.
            unsafe {
                guard.data()?.as_ptr().cast::<f32>().write(42.0);
            }
        }
        assert!(chunk.indices()?.is_some());
        // SAFETY: the mock's `jit_glchunk_copy` is a deep copy and leaves a partial result to us.
        let mut copy = unsafe { chunk.try_clone() }?;
        {
            let mut vertices = copy.vertices()?;
            let mut guard = vertices.lock();
            // SAFETY: same fixture guarantees as above; deep copy has independent storage.
            unsafe {
                let ptr = guard.data()?.as_ptr().cast::<f32>();
                assert_eq!(ptr.read(), 42.0);
                ptr.write(7.0);
            }
        }
        {
            let mut vertices = chunk.vertices()?;
            let mut guard = vertices.lock();
            assert_eq!(unsafe { guard.data()?.as_ptr().cast::<f32>().read() }, 42.0);
        }
        for partial in [false, true] {
            median_mock::jit::fail_next_chunk_copy(partial);
            // SAFETY: as above
            assert!(unsafe { chunk.try_clone() }.is_err());
            assert_eq!(median_mock::jit::live_chunk_count(), 2);
        }
        let mut grid = GLChunk::new_grid(context, GLGridPrim::Quad, 3, 2, 4)?;
        assert_eq!(grid.primitive()?, GLChunkPrim::Grid(GLGridPrim::Quad));
        assert_eq!(grid.vertices()?.lock().info()?.dimensions()?, &[2, 4]);
        // SAFETY: the mock creates initialized CPU matrices with indices 0 in range.
        unsafe {
            chunk.draw()?;
        }
        // `GLGeometry` and `GLDataBuffer` do not exist on Windows (see `median::jit::ob3d`).
        #[cfg(not(target_os = "windows"))]
        {
            assert!(unsafe { GLGeometry::from_raw(context, std::ptr::null_mut()) }.is_err());
            // SAFETY: these CPU fixtures implement only the ownership contract stated by the
            // mock. This does not establish the real SDK's buffer or setdata contract.
            let geometry =
                unsafe { GLGeometry::from_raw(context, median_mock::jit::new_geometry())? };
            unsafe {
                geometry.draw()?;
            }
            let mut payload = [1u8, 2, 3];
            let before = median_mock::jit::buffer_destroy_count();
            let descriptor = median_mock::jit::buffer_descriptor(&mut payload);
            let buffer = unsafe { GLDataBuffer::from_raw(context, descriptor) };
            let mut descriptor = buffer.into_raw();
            assert_eq!(median_mock::jit::buffer_destroy_count(), before);
            unsafe {
                max_sys::jit_gl_buffer_data_destroy_tagged(&mut descriptor);
            }
            assert_eq!(median_mock::jit::buffer_destroy_count(), before + 1);
            let descriptor = median_mock::jit::buffer_descriptor(&mut payload);
            let buffer = unsafe { GLDataBuffer::from_raw(context, descriptor) };
            drop(buffer);
            assert_eq!(median_mock::jit::buffer_destroy_count(), before + 2);
            assert_eq!(payload, [1, 2, 3]);
            let descriptor = median_mock::jit::buffer_descriptor(&mut payload);
            let _buffer = unsafe { GLDataBuffer::from_raw(context, descriptor) };
        }
        assert!(!PANIC.load(Ordering::SeqCst), "test callback cleanup");
        COMPLETED.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}
median::ext_main! { Helpers }

#[test]
fn chunks_copy_draw_and_release_on_success_and_unwind() {
    let k = median_mock::lock();
    unsafe {
        ext_main(std::ptr::null_mut());
    }
    let obj = k.new_object("jit.gl.helper.test", &["render-context".into()]);
    assert!(!obj.is_null());
    let core = median_mock::jit::object_for(obj);
    let (f, _) = k.method(core, "ob3d_draw").unwrap();
    let draw: unsafe extern "C" fn(*mut c_void) -> max_sys::t_jit_err =
        unsafe { std::mem::transmute(f) };
    assert_eq!(unsafe { draw(core) }, 0);
    assert_eq!(COMPLETED.load(Ordering::SeqCst), 1);
    assert_eq!(median_mock::jit::live_chunk_count(), 0);
    assert_eq!(median_mock::jit::live_geometry_count(), 0);
    assert_eq!(median_mock::jit::buffer_destroy_count(), 3);
    PANIC.store(true, Ordering::SeqCst);
    assert_ne!(unsafe { draw(core) }, 0);
    PANIC.store(false, Ordering::SeqCst);
    assert_eq!(median_mock::jit::live_chunk_count(), 0);
    assert_eq!(median_mock::jit::live_geometry_count(), 0);
    assert_eq!(median_mock::jit::buffer_destroy_count(), 6);
    assert!(
        k.take_console_log()
            .iter()
            .any(|e| e.text.contains("test callback cleanup"))
    );
    assert!(k.free_object(obj));
    assert_eq!(median_mock::jit::context_count(), 0);
}
