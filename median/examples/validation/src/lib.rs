//! Diagnostic externals for the accompanying Max patches. No audio-thread logging or allocation.

pub(crate) mod buffer_probe;

use median::{
    dict::{Dict, DictError},
    gimmeback::Reply,
    jit::{
        DrawContext, JitBuilder, JitResult, MatrixOp, Ob3d,
        matrix::{Count, IOCount, Matrix},
        wrapper::{Core, MatrixKind},
    },
    prelude::*,
    thread::SysThread,
};

fn report(obj: &ObjRef, label: &str, event: &str) {
    obj.post(format!(
        "{label}: {event}; main={} scheduler={} audio={}",
        SysThread::is_main(),
        SysThread::is_scheduler(),
        SysThread::is_audio(),
    ));
}

#[derive(MaxObject)]
#[max(name = "median.validate")]
pub struct Messages {
    obj: ObjRef,
    label: String,
}

impl MaxObjWrapped for Messages {
    fn new(b: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        b.inlet(Inlet::default().assist("float, direct, low, front; see Max console"));
        let label = b
            .creation_args()
            .first()
            .map_or_else(|| "messages".to_owned(), |a| a.get_symbol().to_string());
        Ok(Self {
            obj: b.obj(),
            label,
        })
    }
}

#[median::methods]
impl Messages {
    #[loadbang]
    fn loaded(&self) {
        report(&self.obj, &self.label, "loadbang");
    }

    // Deliberately no #[int]: Max rejected integer messages in the DSP-off host test.
    #[float]
    fn number(&self, value: f64) {
        report(&self.obj, &self.label, &format!("float {value}"));
    }

    #[gimme("direct")]
    fn direct(&self, atoms: &[Atom]) {
        self.arguments(&median::sym!("direct"), atoms);
    }

    #[gimme("low")]
    #[defer_low]
    fn low(&self, atoms: &[Atom]) {
        self.arguments(&median::sym!("low"), atoms);
    }

    #[gimme("front")]
    #[defer]
    fn front(&self, atoms: &[Atom]) {
        self.arguments(&median::sym!("front"), atoms);
    }

    fn arguments(&self, sel: &SymbolRef, atoms: &[Atom]) {
        let values = atoms
            .iter()
            .map(|a| match a.get_value() {
                Some(median::atom::AtomValue::Int(v)) => v.to_string(),
                Some(median::atom::AtomValue::Float(v)) => v.to_string(),
                Some(median::atom::AtomValue::Symbol(v)) => v.to_string(),
                _ => "<unsupported atom>".to_owned(),
            })
            .collect::<Vec<_>>()
            .join(" ");
        report(&self.obj, &self.label, &format!("{sel} {values}"));
    }

    // The gimmeback methods are for typed callers (`js`, patch 05): a patch cord cannot reach
    // them (observed in Max 9.2 on 2026-10-27), so median hides them from the patcher. Each posts a line so a run shows the method ran even if the
    // caller drops the reply.

    #[gimmeback("getcount")]
    fn get_count(&self, atoms: &[Atom]) -> t_atom_long {
        report(&self.obj, &self.label, &format!("getcount {}", atoms.len()));
        atoms.len() as t_atom_long
    }

    #[gimmeback("getlist")]
    fn get_list(&self, atoms: &[Atom]) -> Reply {
        report(&self.obj, &self.label, &format!("getlist {}", atoms.len()));
        Reply::Atoms(doubled(atoms))
    }

    #[gimmeback("getstate")]
    fn get_state(&self, atoms: &[Atom]) -> Result<Dict, DictError> {
        report(&self.obj, &self.label, &format!("getstate {}", atoms.len()));
        let d = Dict::new()?;
        d.insert(
            median::sym!("label"),
            SymbolRef::try_from(self.label.as_str()).map_err(|_| DictError::InvalidString)?,
        )?;
        d.insert(median::sym!("count"), atoms.len() as i64)?;
        Ok(d)
    }

    /// A list of 4096 ints (64 KiB of atoms) and no console line, for the leak check: a metro
    /// calling it a hundred times a second would otherwise measure the console, not the reply, and
    /// a reply the caller does not free shows as growth within seconds in Activity Monitor.
    #[gimmeback("getquiet")]
    fn get_quiet(&self, _atoms: &[Atom]) -> Reply {
        Reply::Atoms((0..4096i64).map(Atom::from).collect())
    }

    #[gimmeback("getfail")]
    fn get_fail(&self, atoms: &[Atom]) -> Result<Reply, String> {
        report(&self.obj, &self.label, &format!("getfail {}", atoms.len()));
        Err("getfail: intentional error".into())
    }
}

/// Each int/float doubled, symbols passed through (an `Atom` is not `Clone`).
fn doubled(atoms: &[Atom]) -> Vec<Atom> {
    atoms
        .iter()
        .map(|a| match a.get_value() {
            Some(median::atom::AtomValue::Int(v)) => Atom::from(v * 2),
            Some(median::atom::AtomValue::Float(v)) => Atom::from(v * 2.),
            _ => Atom::from(a.get_symbol()),
        })
        .collect()
}

/// A copy of an int, float or symbol atom.
fn copied(a: &Atom) -> Atom {
    match a.get_value() {
        Some(median::atom::AtomValue::Int(v)) => Atom::from(v),
        Some(median::atom::AtomValue::Float(v)) => Atom::from(v),
        _ => Atom::from(a.get_symbol()),
    }
}

#[derive(MaxObject)]
#[max(name = "median.validate~", kind = msp)]
pub struct Signals {
    obj: ObjRef,
}

impl MSPObjWrapped for Signals {
    fn new(b: &mut MSPBuilder<Self>) -> Result<Self, NewError> {
        b.inlet(Inlet::signal().assist("0: left signal, passed to outlet 0"));
        b.inlet(Inlet::signal().assist("1: right signal, passed to outlet 1"));
        b.inlet(Inlet::int(Self::integer).assist("2: integer, reported in console"));
        b.inlet(Inlet::proxy().assist("3: proxy; send probe"));
        b.outlet(Outlet::signal().assist("0: left signal unchanged"));
        b.outlet(Outlet::signal().assist("1: right signal unchanged"));
        Ok(Self { obj: b.obj() })
    }

    fn perform(&self, ins: &[&[f64]], outs: &mut [&mut [f64]], _: usize) {
        for (input, output) in ins.iter().zip(outs.iter_mut()) {
            output.copy_from_slice(input);
        }
    }
}

#[median::methods]
impl Signals {
    #[sel("probe")]
    fn probe(&self) {
        report(
            &self.obj,
            "signals",
            &format!("proxy inlet {}", self.obj.proxy_inlet()),
        );
    }

    fn integer(&self, value: t_atom_long) {
        report(
            &self.obj,
            "signals",
            &format!("integer inlet 2 value {value}"),
        );
    }
}

#[derive(MaxObject)]
#[max(name = "jit.median.validate", kind = matrix_op)]
pub struct Matrices {
    obj: ObjRef,
    integer: Outlet<Int>,
    done: Outlet<Bang>,
}

impl MatrixOp for Matrices {
    fn io_count() -> IOCount {
        IOCount {
            inputs: Count::Fixed(2),
            outputs: Count::Fixed(2),
        }
    }

    fn new(b: &mut JitBuilder<Self>) -> Result<Self, NewError> {
        let integer = b.outlet(Outlet::int().assist("3: custom integer echo"))?;
        let done = b.outlet(Outlet::bang().assist("4: custom probe bang"))?;
        b.inlet(Inlet::int(Self::integer).assist("2: custom integer, cold"))?;
        b.inlet(
            Inlet::float(Self::number)
                .hot()
                .assist("3: custom float, hot"),
        )?;
        b.inlet(Inlet::proxy().assist("4: custom proxy; send probe"))?;
        if b.creation_args()
            .first()
            .is_some_and(|a| a.get_symbol() == median::sym!("refuse"))
        {
            return Err(
                "validation: intentional constructor refusal after making custom iolets".into(),
            );
        }
        Ok(Self {
            obj: b.max_obj()?,
            integer,
            done,
        })
    }

    // Gimmeback methods on the Jitter class: median then passes the gimmeback wrap flag, so a
    // patcher message gets the reply out the dumpout outlet (SDK position 2, unchanged).
    fn class_setup(c: &mut Class<Core<Self, MatrixKind>>) -> median::error::MaxResult<()> {
        c.add_gimmeback(c"getvalues", Self::values)?;
        c.add_gimmeback(c"getcount", Self::count)?;
        c.add_gimmeback(c"getbig", Self::big)
    }

    // This fixture probes iolet layout, not matrix processing. Do not read its matrix outputs.
    fn calc(&self, _: &mut [Matrix<'_>], _: &mut [Matrix<'_>]) -> JitResult<()> {
        Ok(())
    }
}

impl Matrices {
    fn values(&self, atoms: &[Atom]) -> Reply {
        report(&self.obj, "matrix", &format!("getvalues {}", atoms.len()));
        Reply::Atoms(
            [
                Atom::from(1i64),
                Atom::from(2.5),
                Atom::from(SymbolRef::from(c"foo")),
            ]
            .into_iter()
            .chain(atoms.iter().map(copied))
            .collect(),
        )
    }

    fn count(&self, atoms: &[Atom]) -> t_atom_long {
        report(&self.obj, "matrix", &format!("getcount {}", atoms.len()));
        atoms.len() as t_atom_long
    }

    /// A list of 4096 ints (64 KiB of atoms) and no console line, like `Messages::getquiet`:
    /// patch 05 part C bursts it into a `jit.median.validate` with nothing on its dumpout, so the
    /// memory check measures the reply and not the console.
    fn big(&self, _atoms: &[Atom]) -> Reply {
        Reply::Atoms((0..4096i64).map(Atom::from).collect())
    }
}

#[median::methods]
impl Matrices {
    #[sel("probe")]
    fn probe(&self) -> Result<(), median::outlet::SendError> {
        report(
            &self.obj,
            "matrix",
            &format!("proxy inlet {}", self.obj.proxy_inlet()),
        );
        self.done.send()
    }

    fn integer(&self, value: t_atom_long) {
        if let Err(error) = self.integer.send(value) {
            self.obj.error(format!("integer outlet: {error}"));
        }
    }

    fn number(&self, value: f64) {
        report(&self.obj, "matrix", &format!("float inlet 3 value {value}"));
    }
}

#[derive(MaxObject)]
#[max(name = "jit.gl.median.validate", kind = gl)]
pub struct Graphics {
    obj: ObjRef,
    out: Outlet<Int>,
}

impl Ob3d for Graphics {
    fn new(b: &mut JitBuilder<Self>) -> Result<Self, NewError> {
        b.inlet(Inlet::proxy().hot().assist("1: custom proxy; send probe"))?;
        Ok(Self {
            obj: b.max_obj()?,
            out: b.outlet(Outlet::int().assist("2: custom proxy inlet index"))?,
        })
    }

    // No rendering: this fixture isolates ob3d's iolet layout and context lifecycle.
    fn draw(&self, _: &DrawContext) -> JitResult<()> {
        Ok(())
    }
}

#[median::methods]
impl Graphics {
    #[sel("probe")]
    fn probe(&self) -> Result<(), median::outlet::SendError> {
        let index = self.obj.proxy_inlet();
        report(&self.obj, "gl", &format!("proxy inlet {index}"));
        self.out.send(index as t_atom_long)
    }
}

median::ext_main! { Messages, Signals, Matrices, Graphics, buffer_probe::BufferProbe }
