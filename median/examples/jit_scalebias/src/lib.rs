//! The SDK's ARGB char scalebias operator, using the derive's MOP/Max glue.
//! A per-call input snapshot supports in-place input/output without overlapping Rust borrows.
use median::max_sys::t_jit_error_code as err;
use median::{
    jit::{
        JitBuilder, JitResult, MatrixOp,
        matrix::{Count, IOCount, MOP, Matrix, MatrixDataType},
    },
    prelude::*,
};

#[derive(MaxObject)]
#[max(name = "jit.median.scalebias", kind = matrix_op)]
pub struct ScaleBias {
    #[attr(clip(0, 1), label = "Sum Planes")]
    mode: Int64,
    #[attr(label = "Alpha Scale")]
    ascale: Float32,
    #[attr(label = "Red Scale")]
    rscale: Float32,
    #[attr(label = "Green Scale")]
    gscale: Float32,
    #[attr(label = "Blue Scale")]
    bscale: Float32,
    #[attr(label = "Alpha Bias")]
    abias: Float32,
    #[attr(label = "Red Bias")]
    rbias: Float32,
    #[attr(label = "Green Bias")]
    gbias: Float32,
    #[attr(label = "Blue Bias")]
    bbias: Float32,
}
impl MatrixOp for ScaleBias {
    fn new(_: &mut JitBuilder) -> Result<Self, NewError> {
        Ok(Self {
            mode: 0i64.into(),
            ascale: 1.0.into(),
            rscale: 1.0.into(),
            gscale: 1.0.into(),
            bscale: 1.0.into(),
            abias: 0.0.into(),
            rbias: 0.0.into(),
            gbias: 0.0.into(),
            bbias: 0.0.into(),
        })
    }
    fn io_count() -> IOCount {
        IOCount {
            inputs: Count::Fixed(1),
            outputs: Count::Fixed(1),
        }
    }
    fn mop_setup(mop: &mut MOP<'_>) -> JitResult<()> {
        mop.single_type(MatrixDataType::Char)?;
        mop.single_plane_count(4)
    }
    fn calc(&self, inputs: &mut [Matrix<'_>], outputs: &mut [Matrix<'_>]) -> JitResult<()> {
        let input = inputs.first_mut().ok_or(err::JIT_ERR_INVALID_PTR)?;
        let output = outputs.first_mut().ok_or(err::JIT_ERR_INVALID_PTR)?;
        let (input_dims, pixels) = {
            let mut lock = input.lock();
            let data = lock.data()?;
            if data.info().planes()? != 4 {
                return Err(err::JIT_ERR_MISMATCH_PLANE);
            }
            let dims: Vec<_> = data
                .info()
                .dimensions()?
                .iter()
                .map(|d| *d as usize)
                .collect();
            let mut pixels = Vec::<[u8; 4]>::new();
            // SAFETY: the SDK's matrix_calc caller supplies a stable, initialized input while
            // calculating. Copy before taking any output borrow, including when in-place.
            // As with the SDK C example, concurrent external writers must obey host scheduling.
            for chunk in unsafe { data.chunks::<u8>()? } {
                for cell in chunk.entry_iter() {
                    pixels.push(cell.try_into().map_err(|_| err::JIT_ERR_MISMATCH_PLANE)?);
                }
            }
            (dims, pixels)
        };
        let scales = [&self.ascale, &self.rscale, &self.gscale, &self.bscale]
            .map(|v| (v.get() as f64 * 256.0) as i64);
        let bias_values = [&self.abias, &self.rbias, &self.gbias, &self.bbias].map(Float32::get);
        let biases = bias_values.map(|v| (v as f64 * 256.0) as i64);
        let sum_bias = (bias_values.iter().sum::<f32>() as f64 * 256.0) as i64;
        let sum = self.mode.get() != 0;
        let mut lock = output.lock();
        let mut data = lock.data()?;
        if data.info().planes()? != 4 {
            return Err(err::JIT_ERR_MISMATCH_PLANE);
        }
        let dims: Vec<_> = data
            .info()
            .dimensions()?
            .iter()
            .map(|d| *d as usize)
            .collect();
        if dims.len() != input_dims.len() {
            return Err(err::JIT_ERR_MISMATCH_DIM);
        }
        let mut linear = 0usize;
        // SAFETY: this output belongs to this matrix_calc call. The input snapshot above has
        // ended every input-data borrow, so an in-place output cannot alias those references.
        for mut chunk in unsafe { data.chunks_mut::<u8>()? } {
            for out in chunk.entry_iter_mut() {
                let mut coordinate = linear;
                let (mut source, mut stride, mut inside) = (0usize, 1usize, true);
                for (&dim, &input_dim) in dims.iter().zip(&input_dims) {
                    let index = coordinate % dim;
                    coordinate /= dim;
                    if index >= input_dim {
                        inside = false;
                        break;
                    }
                    source = source
                        .checked_add(
                            index
                                .checked_mul(stride)
                                .ok_or(err::JIT_ERR_OUT_OF_BOUNDS)?,
                        )
                        .ok_or(err::JIT_ERR_OUT_OF_BOUNDS)?;
                    stride = stride
                        .checked_mul(input_dim)
                        .ok_or(err::JIT_ERR_OUT_OF_BOUNDS)?;
                }
                linear += 1;
                if !inside {
                    continue;
                } // SDK behavior: process the dimension intersection.
                let pixel = pixels.get(source).ok_or(err::JIT_ERR_OUT_OF_BOUNDS)?;
                if sum {
                    let scaled = pixel.iter().zip(scales).fold(0i64, |v, (&p, s)| {
                        v.saturating_add((p as i64).saturating_mul(s))
                    });
                    out.fill((scaled >> 8).saturating_add(sum_bias).clamp(0, 255) as u8);
                } else {
                    for i in 0..4 {
                        out[i] = (((pixel[i] as i64).saturating_mul(scales[i])) >> 8)
                            .saturating_add(biases[i])
                            .clamp(0, 255) as u8;
                    }
                }
            }
        }
        Ok(())
    }
}
#[median::methods]
impl ScaleBias {
    #[sel("scale")]
    fn scale(&self, value: f64) {
        for channel in [&self.ascale, &self.rscale, &self.gscale, &self.bscale] {
            channel.set(value as f32);
        }
    }
    #[sel("bias")]
    fn bias(&self, value: f64) {
        for channel in [&self.abias, &self.rbias, &self.gbias, &self.bbias] {
            channel.set(value as f32);
        }
    }
}
median::ext_main! { ScaleBias }
