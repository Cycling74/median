//! Method encapsulation and type aliases
use crate::{
    atom::Atom,
    defer::{Defer, DeferredCall, OwnedAtoms},
    notify::Notification,
    object::MaxObj,
    symbol::SymbolRef,
    wrapper::WrapperWrapped,
};
use std::{ffi::c_void, fmt::Display, os::raw::c_long};

pub type MaxNew = unsafe extern "C" fn(
    sym: *mut max_sys::t_symbol,
    argc: c_long,
    argv: *const max_sys::t_atom,
) -> *mut c_void;
pub type MaxFree<T> = unsafe extern "C" fn(obj: *mut T);

// The pinned SDK declares `method` as this erased pointer on every supported 64-bit target.
// Actual calls use the registered trampoline's signature (including its floating arguments).
pub type MaxMethod = unsafe extern "C" fn(arg1: *mut c_void) -> *mut c_void;

/// helper method to convert between max and median calls selector list method calls
///
/// # Safety
/// * `sym` must be null or point to a valid `t_symbol`.
/// * If `ac > 0`, `av` must point to `ac` valid, initialized atoms that live for the duration of
///   the call.
///
/// These are the guarantees Max makes when it calls a method registered with `A_GIMME`, which is
/// the only intended caller.
pub unsafe fn sel_list<F>(
    sym: *mut max_sys::t_symbol,
    ac: ::std::os::raw::c_long,
    av: *const ::max_sys::t_atom,
    f: F,
) where
    F: Fn(SymbolRef, &[Atom]),
{
    // SAFETY: the caller guarantees `sym` is null or valid
    let sym = unsafe { SymbolRef::from_raw(sym) };
    let atoms = if ac > 0 && !av.is_null() {
        // SAFETY: the caller guarantees `av` points to `ac` atoms, and Atom is transparent
        unsafe {
            std::slice::from_raw_parts(
                std::mem::transmute::<*const ::max_sys::t_atom, *const Atom>(av),
                ac as _,
            )
        }
    } else {
        &[]
    };
    f(sym, atoms);
}

//
// Typed, macro-free method registration (see `Class::add_bang` and friends).
//
// The user passes a zero-sized `Copy` callable (a fn item or a non-capturing closure). Its type
// is the only thing the trampoline needs: the trampoline is a generic `extern "C" fn` that
// fabricates the callable with `zst` and calls it with the converted arguments, inside the
// panic guard.
//

/// Fabricate a value of the zero-sized type `F`, which is how a trampoline gets hold of the
/// callable that was passed at registration time: there is nowhere to store it, and a zero-sized
/// type carries no data, so a "new" value is as good as the original.
///
/// `F: Copy` is required (and not just `size_of::<F>() == 0`) because zero size alone doesn't
/// rule out a closure that captured a zero-sized value with a `Drop` impl. Fabricating such a
/// closure and dropping it after every call would run that `Drop` again and again for a value
/// that was never created. `Copy` types can't have a `Drop` impl, closures are `Copy` only when
/// all of their captures are, and fn items are always `Copy`, so `F: Fn(..) + Copy` is exactly a
/// "stateless callable".
///
/// A callable that is not zero-sized (a closure that captures something) is rejected when the
/// trampoline is instantiated, with a compile error.
///
/// # Safety
/// `F` must be a type that can be created from nothing: it can be zero-sized and `Copy` (checked
/// here) but must also have no invariant that its single value must be created by some
/// particular code. That holds for fn items and closures without captures.
pub(crate) unsafe fn zst<F: Copy>() -> F {
    const {
        assert!(
            std::mem::size_of::<F>() == 0,
            "callback must be a non-capturing closure or fn item"
        )
    };
    // SAFETY: `F` is zero-sized, so there are no bytes to be uninitialised, and the caller
    // guarantees that it has no further invariant. (The lint is about types that have bytes.)
    #[allow(clippy::uninit_assumed_init)]
    unsafe {
        std::mem::MaybeUninit::<F>::uninit().assume_init()
    }
}

/// Erase the type of a trampoline's fn pointer so it can be passed to `class_addmethod`.
///
/// # Safety
/// `P` must be an `extern "C"` fn pointer type, whose first argument is the object pointer.
pub(crate) unsafe fn erase<P: Copy>(p: P) -> MaxMethod {
    const {
        assert!(std::mem::size_of::<P>() == std::mem::size_of::<MaxMethod>());
    };
    // SAFETY: the caller guarantees that `P` is a fn pointer, which has the size (checked
    // above) and representation of any other. Max calls it with the arguments registered.
    unsafe { std::mem::transmute_copy::<P, MaxMethod>(&p) }
}

mod sealed {
    pub trait Sealed {}
    impl Sealed for () {}
    impl<E> Sealed for Result<(), E> {}

    /// Seals `SelArg`: its `C` and `ATOM_TYPE` define the ABI of the trampoline, so an impl
    /// outside this crate could make Max pass one type where the trampoline reads another.
    pub trait SealedArg {}
    impl SealedArg for f64 {}
    impl SealedArg for max_sys::t_atom_long {}
    impl SealedArg for crate::symbol::SymbolRef {}

    /// Seals `SelFn`: implemented exactly where `SelFn` is, by `sel_fn_arity!`.
    pub trait SealedSelFn<T, Args, R> {}

    /// Seals `GimmeFn`: implemented exactly where `GimmeFn` is.
    pub trait SealedGimmeFn<T, R, A> {}
}

/// What a method registered with `Class::add_*` may return: `()` or `Result<(), E>` where
/// `E: Display`. On `Err` the trampoline calls the type's `report_error` hook
/// ([`MaxObjWrapped::report_error`](crate::wrapper::MaxObjWrapped::report_error)), which by
/// default posts the error to the Max console for the object, so `?` works in methods.
///
/// This trait is sealed.
#[diagnostic::on_unimplemented(
    message = "a median method must return `()` or `Result<(), E>` where `E: Display`, not `{Self}`",
    label = "returns `{Self}`"
)]
pub trait MethodResult: sealed::Sealed {
    /// If this is an error, give it to `report`.
    #[doc(hidden)]
    fn report_if_err(self, report: &mut dyn FnMut(&dyn Display));
}

impl MethodResult for () {
    fn report_if_err(self, _report: &mut dyn FnMut(&dyn Display)) {}
}

impl<E: Display> MethodResult for Result<(), E> {
    fn report_if_err(self, report: &mut dyn FnMut(&dyn Display)) {
        if let Err(e) = self {
            report(&e);
        }
    }
}

/// Run `f` on the object's `T` inside the panic guard, giving an `Err` result to the wrapped
/// type's `report_error` hook (through [`WrapperWrapped::report_error`]).
fn run<W, T, R>(w: &W, what: &str, f: impl FnOnce(&T) -> R)
where
    W: WrapperWrapped<T> + MaxObj,
    R: MethodResult,
{
    crate::ffi::guard(w, what, || {
        f(w.wrapped()).report_if_err(&mut |err| w.report_error(err));
    })
}

pub(crate) extern "C" fn dictionary_tramp<W, T, F, R>(w: &W, name: *mut max_sys::t_symbol)
where
    W: WrapperWrapped<T> + MaxObj,
    F: Fn(&T, crate::dict::Dict) -> R + Copy,
    R: MethodResult,
{
    crate::ffi::guard(w, "dictionary", || {
        let name = unsafe { SymbolRef::from_raw(name) };
        match crate::dict::Dict::find_registered(name) {
            Some(d) => (unsafe { zst::<F>() })(w.wrapped(), d)
                .report_if_err(&mut |err| w.report_error(err)),
            None => w.report_error(&format!("unable to reference dictionary named {name}")),
        }
    });
}

pub(crate) extern "C" fn savestate_tramp<W, T, F, R>(w: &W, ptr: *mut max_sys::t_dictionary)
where
    W: WrapperWrapped<T> + MaxObj,
    F: Fn(&T, &crate::dict::Dict) -> R + Copy,
    R: MethodResult,
{
    crate::ffi::guard(w, "appendtodictionary", || {
        // Max keeps the patcher's dictionary alive for this call. A temporary retain also
        // allows the user to clone the handle without borrowing storage from the callback.
        match unsafe { crate::dict::Dict::from_raw(ptr, crate::dict::Ownership::Borrowed) } {
            Ok(d) => (unsafe { zst::<F>() })(w.wrapped(), &d)
                .report_if_err(&mut |err| w.report_error(err)),
            Err(err) => w.report_error(&err),
        }
    });
}

/// Marker for [`GimmeFn`]: the callable takes the selector, `Fn(&T, &SymbolRef, &[Atom]) -> R`.
pub struct WithSel;

/// Marker for [`GimmeFn`]: the callable does not take the selector, `Fn(&T, &[Atom]) -> R`.
pub struct NoSel;

/// A callable that can be registered as an `A_GIMME` selector method with
/// [`Class::add_sel_gimme`](crate::class::Class::add_sel_gimme): a zero-sized `Copy`
/// `Fn(&T, &[Atom]) -> R` or `Fn(&T, &SymbolRef, &[Atom]) -> R`.
///
/// A method that takes the selector can serve several selectors (it is told which one was sent);
/// one that does not is the form `#[sel_gimme("name")]` uses, since the marker already names the
/// selector. Implemented for all such callables, not meant to be implemented by hand (it is
/// sealed).
///
/// `A` is [`WithSel`] or [`NoSel`], picked by the callable's signature and inferred, so callers
/// never name it. The two impls can't overlap because `A` differs.
///
/// A fn item (`Self::read`) needs nothing more. A closure must annotate its parameters
/// (`|_: &T, _: &[Atom]| ..`): the compiler infers closure parameter types from a plain `Fn`
/// bound, not through this trait, so `|_, _| ..` does not type-check.
#[diagnostic::on_unimplemented(
    message = "`{Self}` can't be an `A_GIMME` method of `{T}`",
    label = "expected `fn(&{T}, &[Atom])` or `fn(&{T}, &SymbolRef, &[Atom])`, returning `()` or `Result<(), E>`",
    note = "the callable must be a fn item or a closure that captures nothing; a closure must annotate its parameter types (`|_: &T, _: &[Atom]| ..`)"
)]
pub trait GimmeFn<T, R, A>: Copy + Send + Sync + 'static + sealed::SealedGimmeFn<T, R, A> {
    /// Call the method; the `NoSel` impl ignores `sel`.
    #[doc(hidden)]
    fn call(self, t: &T, sel: &SymbolRef, atoms: &[Atom]) -> R;
}

impl<T, R, F> sealed::SealedGimmeFn<T, R, WithSel> for F where
    F: Fn(&T, &SymbolRef, &[Atom]) -> R + Copy + Send + Sync + 'static
{
}

impl<T, R, F> GimmeFn<T, R, WithSel> for F
where
    F: Fn(&T, &SymbolRef, &[Atom]) -> R + Copy + Send + Sync + 'static,
{
    fn call(self, t: &T, sel: &SymbolRef, atoms: &[Atom]) -> R {
        self(t, sel, atoms)
    }
}

impl<T, R, F> sealed::SealedGimmeFn<T, R, NoSel> for F where
    F: Fn(&T, &[Atom]) -> R + Copy + Send + Sync + 'static
{
}

impl<T, R, F> GimmeFn<T, R, NoSel> for F
where
    F: Fn(&T, &[Atom]) -> R + Copy + Send + Sync + 'static,
{
    fn call(self, t: &T, _sel: &SymbolRef, atoms: &[Atom]) -> R {
        self(t, atoms)
    }
}

/// Build a `&[Atom]` from the `argc`/`argv` Max passes to an `A_GIMME` method.
///
/// # Safety
/// If `ac > 0`, `av` must point to `ac` valid atoms that live as long as `'a`.
unsafe fn atoms<'a>(ac: c_long, av: *const max_sys::t_atom) -> &'a [Atom] {
    if ac > 0 && !av.is_null() {
        // SAFETY: guaranteed by the caller, and `Atom` is `repr(transparent)` over `t_atom`
        unsafe { std::slice::from_raw_parts(av.cast::<Atom>(), ac as usize) }
    } else {
        &[]
    }
}

pub(crate) extern "C" fn bang_tramp<W, T, F, R>(w: &W)
where
    W: WrapperWrapped<T> + MaxObj,
    F: Fn(&T) -> R + Copy,
    R: MethodResult,
{
    // SAFETY: `F` is a zero-sized `Copy` callable, see `zst`
    run(w, "bang", |t| (unsafe { zst::<F>() })(t));
}

pub(crate) extern "C" fn dblclick_tramp<W, T, F, R>(w: &W)
where
    W: WrapperWrapped<T> + MaxObj,
    F: Fn(&T) -> R + Copy,
    R: MethodResult,
{
    // SAFETY: `F` is a zero-sized `Copy` callable, see `zst`
    run(w, "dblclick", |t| (unsafe { zst::<F>() })(t));
}

pub(crate) extern "C" fn notify_tramp<W, T, F, R>(
    w: &W,
    sender_name: *mut max_sys::t_symbol,
    message: *mut max_sys::t_symbol,
    sender: *mut c_void,
    data: *mut c_void,
) where
    W: WrapperWrapped<T> + MaxObj,
    F: Fn(&T, &Notification) -> R + Copy,
    R: MethodResult,
{
    crate::ffi::guard(w, "notify", || {
        // SAFETY: Max passes valid (or null) symbols to a notify method, and its other arguments
        // as they are
        let notification = unsafe { Notification::new(sender_name, message, sender, data) };
        // median's own handling (the object's `buffer~` references) first
        w.notify_buffer_refs(&notification);
        // SAFETY: `F` is a zero-sized `Copy` callable, see `zst`
        (unsafe { zst::<F>() })(w.wrapped(), &notification)
            .report_if_err(&mut |err| w.report_error(err));
    });
}

pub(crate) extern "C" fn loadbang_tramp<W, T, F, R>(w: &W)
where
    W: WrapperWrapped<T> + MaxObj,
    F: Fn(&T) -> R + Copy,
    R: MethodResult,
{
    // SAFETY: `F` is a zero-sized `Copy` callable, see `zst`
    run(w, "loadbang", |t| (unsafe { zst::<F>() })(t));
}

pub(crate) extern "C" fn int_tramp<W, T, F, R>(w: &W, v: max_sys::t_atom_long)
where
    W: WrapperWrapped<T> + MaxObj,
    F: Fn(&T, max_sys::t_atom_long) -> R + Copy,
    R: MethodResult,
{
    // SAFETY: `F` is a zero-sized `Copy` callable, see `zst`
    run(w, "int", |t| (unsafe { zst::<F>() })(t, v));
}

pub(crate) extern "C" fn float_tramp<W, T, F, R>(w: &W, v: f64)
where
    W: WrapperWrapped<T> + MaxObj,
    F: Fn(&T, f64) -> R + Copy,
    R: MethodResult,
{
    // SAFETY: `F` is a zero-sized `Copy` callable, see `zst`
    run(w, "float", |t| (unsafe { zst::<F>() })(t, v));
}

pub(crate) extern "C" fn sym_tramp<W, T, F, R>(w: &W, s: *mut max_sys::t_symbol)
where
    W: WrapperWrapped<T> + MaxObj,
    F: Fn(&T, SymbolRef) -> R + Copy,
    R: MethodResult,
{
    // SAFETY: `F` is a zero-sized `Copy` callable, see `zst`; Max passes a valid symbol (or
    // null, which `from_raw` maps to the empty symbol) for an `A_SYM` argument
    run(w, "symbol", |t| {
        (unsafe { zst::<F>() })(t, unsafe { SymbolRef::from_raw(s) })
    });
}

pub(crate) extern "C" fn list_tramp<W, T, F, R>(
    w: &W,
    _sel: *mut max_sys::t_symbol,
    ac: c_long,
    av: *const max_sys::t_atom,
) where
    W: WrapperWrapped<T> + MaxObj,
    F: Fn(&T, &[Atom]) -> R + Copy,
    R: MethodResult,
{
    // SAFETY: `F` is a zero-sized `Copy` callable, see `zst`; Max passes `ac` valid atoms
    // that live for the call for an `A_GIMME` method
    run(w, "list", |t| {
        (unsafe { zst::<F>() })(t, unsafe { atoms(ac, av) })
    });
}

pub(crate) extern "C" fn any_tramp<W, T, F, R>(
    w: &W,
    sel: *mut max_sys::t_symbol,
    ac: c_long,
    av: *const max_sys::t_atom,
) where
    W: WrapperWrapped<T> + MaxObj,
    F: Fn(&T, &SymbolRef, &[Atom]) -> R + Copy,
    R: MethodResult,
{
    // SAFETY: `F` is a zero-sized `Copy` callable, see `zst`; Max passes a valid selector and
    // `ac` valid atoms that live for the call for an `A_GIMME` method
    run(w, "anything", |t| {
        let sel = unsafe { SymbolRef::from_raw(sel) };
        (unsafe { zst::<F>() })(t, &sel, unsafe { atoms(ac, av) })
    });
}

/// A type that can be an argument of a typed selector method (`Class::add_sel`): `f64`
/// (`A_FLOAT`), `t_atom_long` (`A_LONG`) or [`SymbolRef`] (`A_SYM`).
///
/// This trait is sealed: the associated items say how Max passes the argument, so an impl for
/// another type could make the trampoline misread it.
pub trait SelArg: Copy + Send + 'static + sealed::SealedArg {
    /// How Max passes the argument to the method.
    type C;
    /// The atom type to register the argument as.
    const ATOM_TYPE: max_sys::e_max_atomtypes::Type;
    /// Convert from the argument as Max passed it.
    ///
    /// # Safety
    /// `c` must be what Max passed for an argument registered as `ATOM_TYPE`.
    unsafe fn from_c(c: Self::C) -> Self;
}

impl SelArg for f64 {
    type C = f64;
    const ATOM_TYPE: max_sys::e_max_atomtypes::Type = max_sys::e_max_atomtypes::A_FLOAT;
    unsafe fn from_c(c: f64) -> f64 {
        c
    }
}

impl SelArg for max_sys::t_atom_long {
    type C = max_sys::t_atom_long;
    const ATOM_TYPE: max_sys::e_max_atomtypes::Type = max_sys::e_max_atomtypes::A_LONG;
    unsafe fn from_c(c: max_sys::t_atom_long) -> max_sys::t_atom_long {
        c
    }
}

impl SelArg for SymbolRef {
    type C = *mut max_sys::t_symbol;
    const ATOM_TYPE: max_sys::e_max_atomtypes::Type = max_sys::e_max_atomtypes::A_SYM;
    unsafe fn from_c(c: *mut max_sys::t_symbol) -> SymbolRef {
        // SAFETY: the caller guarantees `c` is what Max passed for an `A_SYM` argument: a valid
        // symbol (null is mapped to the empty symbol)
        unsafe { SymbolRef::from_raw(c) }
    }
}

/// A callable that can be registered as a typed selector method with `Class::add_sel`: a
/// zero-sized `Copy` `Fn(&T, A1, .., An) -> R`, with up to 7 arguments that are each `f64`,
/// `t_atom_long` or [`SymbolRef`]. The arguments are either all `f64` or none of them are: Max
/// can't pass a mix (the SDK says to use `A_GIMME`), so registering one is a compile error. Implemented for all such callables, not meant to be
/// implemented by hand (it is sealed).
///
/// `Args` is the tuple of the argument types and `R` the return type; they only exist to let
/// one impl per arity coexist, and are inferred.
pub trait SelFn<T, Args, R>:
    Copy + Send + Sync + 'static + sealed::SealedSelFn<T, Args, R>
{
    /// The atom type of each argument.
    #[doc(hidden)]
    const TYPES: &'static [max_sys::e_max_atomtypes::Type];

    /// The trampoline for this callable on the wrapper `W`, as registered with Max.
    #[doc(hidden)]
    fn trampoline<W>() -> MaxMethod
    where
        W: WrapperWrapped<T> + MaxObj;

    /// The deferring trampoline for this callable on the wrapper `W` (see [`crate::defer`]).
    #[doc(hidden)]
    fn deferred_trampoline<W>(mode: Defer) -> MaxMethod
    where
        W: WrapperWrapped<T> + MaxObj,
        T: 'static;
}

/// Whether `types` has both `A_FLOAT` and other types. Max's typed dispatch passes the arguments
/// of a method either all as doubles or all as pointer-sized words, so a method that takes both
/// gets its floats (or the others) from the wrong registers on x64. The SDK says to use
/// `A_GIMME` for such a method.
pub(crate) const fn mixes_float(types: &[max_sys::e_max_atomtypes::Type]) -> bool {
    let (mut float, mut other) = (false, false);
    let mut i = 0;
    while i < types.len() {
        if types[i] == max_sys::e_max_atomtypes::A_FLOAT {
            float = true;
        } else {
            other = true;
        }
        i += 1;
    }
    float && other
}

macro_rules! sel_fn_arity {
    ($tramp:ident, $dtramp:ident; $($A:ident $a:ident),*) => {
        extern "C" fn $dtramp<W, T, F, R, $($A,)* const LOW: bool>(w: &W, $($a: <$A as SelArg>::C),*)
        where
            W: WrapperWrapped<T> + MaxObj,
            T: 'static,
            F: Fn(&T, $($A),*) -> R + Copy + Send + 'static,
            R: MethodResult,
            $($A: SelArg),*
        {
            // SAFETY: Max passes the arguments as they were registered (`SelArg::ATOM_TYPE`)
            $(let $a = unsafe { <$A as SelArg>::from_c($a) };)*
            // SAFETY: `F` is a zero-sized `Copy` callable, see `zst`
            defer_or_run::<W, T, R, _, LOW>(w, "method", move |t: &T| {
                (unsafe { zst::<F>() })(t, $($a),*)
            });
        }

        extern "C" fn $tramp<W, T, F, R, $($A),*>(w: &W, $($a: <$A as SelArg>::C),*)
        where
            W: WrapperWrapped<T> + MaxObj,
            F: Fn(&T, $($A),*) -> R + Copy,
            R: MethodResult,
            $($A: SelArg),*
        {
            // SAFETY: `F` is a zero-sized `Copy` callable, see `zst`; Max passes the arguments
            // as they were registered (`SelArg::ATOM_TYPE`)
            run(w, "method", |t| {
                (unsafe { zst::<F>() })(t, $(unsafe { <$A as SelArg>::from_c($a) }),*)
            });
        }

        impl<T, F, R, $($A),*> sealed::SealedSelFn<T, ($($A,)*), R> for F
        where
            F: Fn(&T, $($A),*) -> R + Copy + Send + Sync + 'static,
            R: MethodResult,
            $($A: SelArg),*
        {
        }

        impl<T, F, R, $($A),*> SelFn<T, ($($A,)*), R> for F
        where
            F: Fn(&T, $($A),*) -> R + Copy + Send + Sync + 'static,
            R: MethodResult,
            $($A: SelArg),*
        {
            const TYPES: &'static [max_sys::e_max_atomtypes::Type] = &[$(<$A as SelArg>::ATOM_TYPE),*];

            fn trampoline<W>() -> MaxMethod
            where
                W: WrapperWrapped<T> + MaxObj,
            {
                const {
                    assert!(
                        !mixes_float(Self::TYPES),
                        "a typed selector cannot mix f64 with other argument types (Max passes them in different registers); use #[sel_gimme(\"name\")] / add_sel_gimme"
                    )
                };
                let p: extern "C" fn(&W, $(<$A as SelArg>::C),*) = $tramp::<W, T, F, R, $($A),*>;
                // SAFETY: `p` is an `extern "C"` fn whose first argument is the object, which
                // is how Max calls a method registered with the argument types in `TYPES`
                unsafe { erase(p) }
            }

            fn deferred_trampoline<W>(mode: Defer) -> MaxMethod
            where
                W: WrapperWrapped<T> + MaxObj,
                T: 'static,
            {
                const {
                    assert!(
                        !mixes_float(Self::TYPES),
                        "a typed selector cannot mix f64 with other argument types (Max passes them in different registers); use #[sel_gimme(\"name\")] / add_sel_gimme"
                    )
                };
                let p: extern "C" fn(&W, $(<$A as SelArg>::C),*) = match mode {
                    Defer::Low => $dtramp::<W, T, F, R, $($A,)* true>,
                    Defer::Front => $dtramp::<W, T, F, R, $($A,)* false>,
                };
                // SAFETY: as for `trampoline`
                unsafe { erase(p) }
            }
        }
    };
}

sel_fn_arity!(sel_tramp0, sel_deferred_tramp0;);
sel_fn_arity!(sel_tramp1, sel_deferred_tramp1; A1 a1);
sel_fn_arity!(sel_tramp2, sel_deferred_tramp2; A1 a1, A2 a2);
sel_fn_arity!(sel_tramp3, sel_deferred_tramp3; A1 a1, A2 a2, A3 a3);
sel_fn_arity!(sel_tramp4, sel_deferred_tramp4; A1 a1, A2 a2, A3 a3, A4 a4);
sel_fn_arity!(sel_tramp5, sel_deferred_tramp5; A1 a1, A2 a2, A3 a3, A4 a4, A5 a5);
sel_fn_arity!(sel_tramp6, sel_deferred_tramp6; A1 a1, A2 a2, A3 a3, A4 a4, A5 a5, A6 a6);
sel_fn_arity!(sel_tramp7, sel_deferred_tramp7; A1 a1, A2 a2, A3 a3, A4 a4, A5 a5, A6 a6, A7 a7);

pub(crate) extern "C" fn sel_gimme_tramp<W, T, F, R, A>(
    w: &W,
    sel: *mut max_sys::t_symbol,
    ac: c_long,
    av: *const max_sys::t_atom,
) where
    W: WrapperWrapped<T> + MaxObj,
    F: GimmeFn<T, R, A>,
    R: MethodResult,
{
    // SAFETY: as `any_tramp`
    run(w, "method", |t| {
        let sel = unsafe { SymbolRef::from_raw(sel) };
        F::call(unsafe { zst::<F>() }, t, &sel, unsafe { atoms(ac, av) })
    });
}

// The trampoline of each typed method as Max wants it. `Class::add_*` registers these, and the
// tests call them without a class.

pub(crate) fn bang_method<W, T, F, R>(_f: F) -> MaxMethod
where
    W: WrapperWrapped<T> + MaxObj,
    F: Fn(&T) -> R + Copy,
    R: MethodResult,
{
    // SAFETY: the trampoline takes the object, as registered for `bang`
    unsafe { erase(bang_tramp::<W, T, F, R> as extern "C" fn(&W)) }
}

pub(crate) fn dblclick_method<W, T, F, R>(_f: F) -> MaxMethod
where
    W: WrapperWrapped<T> + MaxObj,
    F: Fn(&T) -> R + Copy,
    R: MethodResult,
{
    // SAFETY: the trampoline takes the object, as Max calls the `A_CANT` method `dblclick`
    unsafe { erase(dblclick_tramp::<W, T, F, R> as extern "C" fn(&W)) }
}

pub(crate) fn notify_method<W, T, F, R>(_f: F) -> MaxMethod
where
    W: WrapperWrapped<T> + MaxObj,
    F: Fn(&T, &Notification) -> R + Copy,
    R: MethodResult,
{
    // SAFETY: the trampoline takes the object and the four arguments Max calls `notify` with
    unsafe {
        erase(
            notify_tramp::<W, T, F, R>
                as extern "C" fn(
                    &W,
                    *mut max_sys::t_symbol,
                    *mut max_sys::t_symbol,
                    *mut c_void,
                    *mut c_void,
                ),
        )
    }
}

pub(crate) fn loadbang_method<W, T, F, R>(_f: F) -> MaxMethod
where
    W: WrapperWrapped<T> + MaxObj,
    F: Fn(&T) -> R + Copy,
    R: MethodResult,
{
    // SAFETY: the trampoline takes the object, as Max calls the `A_CANT` method `loadbang`
    unsafe { erase(loadbang_tramp::<W, T, F, R> as extern "C" fn(&W)) }
}

pub(crate) fn int_method<W, T, F, R>(_f: F) -> MaxMethod
where
    W: WrapperWrapped<T> + MaxObj,
    F: Fn(&T, max_sys::t_atom_long) -> R + Copy,
    R: MethodResult,
{
    // SAFETY: the trampoline takes the object and a `t_atom_long`, as registered for `int`
    unsafe { erase(int_tramp::<W, T, F, R> as extern "C" fn(&W, max_sys::t_atom_long)) }
}

pub(crate) fn float_method<W, T, F, R>(_f: F) -> MaxMethod
where
    W: WrapperWrapped<T> + MaxObj,
    F: Fn(&T, f64) -> R + Copy,
    R: MethodResult,
{
    // SAFETY: the trampoline takes the object and a double, as registered for `float`
    unsafe { erase(float_tramp::<W, T, F, R> as extern "C" fn(&W, f64)) }
}

pub(crate) fn sym_method<W, T, F, R>(_f: F) -> MaxMethod
where
    W: WrapperWrapped<T> + MaxObj,
    F: Fn(&T, SymbolRef) -> R + Copy,
    R: MethodResult,
{
    // SAFETY: the trampoline takes the object and a symbol, as registered for `symbol`
    unsafe { erase(sym_tramp::<W, T, F, R> as extern "C" fn(&W, *mut max_sys::t_symbol)) }
}

pub(crate) fn list_method<W, T, F, R>(_f: F) -> MaxMethod
where
    W: WrapperWrapped<T> + MaxObj,
    F: Fn(&T, &[Atom]) -> R + Copy,
    R: MethodResult,
{
    // SAFETY: the trampoline takes the object and a selector and atoms, as registered for an
    // `A_GIMME` method
    unsafe { erase(list_tramp::<W, T, F, R> as GimmeTramp<W>) }
}

pub(crate) fn any_method<W, T, F, R>(_f: F) -> MaxMethod
where
    W: WrapperWrapped<T> + MaxObj,
    F: Fn(&T, &SymbolRef, &[Atom]) -> R + Copy,
    R: MethodResult,
{
    // SAFETY: as `list_method`
    unsafe { erase(any_tramp::<W, T, F, R> as GimmeTramp<W>) }
}

pub(crate) fn sel_gimme_method<W, T, F, R, A>(_f: F) -> MaxMethod
where
    W: WrapperWrapped<T> + MaxObj,
    F: GimmeFn<T, R, A>,
    R: MethodResult,
{
    // SAFETY: as `list_method`
    unsafe { erase(sel_gimme_tramp::<W, T, F, R, A> as GimmeTramp<W>) }
}

/// The signature of a trampoline for an `A_GIMME` method.
type GimmeTramp<W> = extern "C" fn(&W, *mut max_sys::t_symbol, c_long, *const max_sys::t_atom);

//
// Deferred methods (see `crate::defer`): the trampolines copy the arguments and either run the
// method at once (`Defer::Front` on the main thread) or queue it on the object's deferred-call
// queue. `LOW` is `true` for `Defer::Low`.
//

/// Run `call` now if `LOW` is false and this is the main thread, otherwise queue it on the
/// object's deferred-call queue. Inside the panic guard; an `Err` goes to `report_error` when
/// the call runs.
fn defer_or_run<W, T, R, C, const LOW: bool>(w: &W, what: &'static str, call: C)
where
    W: WrapperWrapped<T> + MaxObj,
    T: 'static,
    R: MethodResult,
    C: FnOnce(&T) -> R + Send + 'static,
{
    crate::ffi::guard(w, what, || {
        if !LOW && crate::thread::SysThread::is_main() {
            call(w.wrapped()).report_if_err(&mut |err| w.report_error(err));
        } else {
            let mode = if LOW { Defer::Low } else { Defer::Front };
            w.defer_call(
                mode,
                DeferredCall::new(what, move |t: &T, report: &mut dyn FnMut(&dyn Display)| {
                    call(t).report_if_err(report)
                }),
            );
        }
    })
}

extern "C" fn bang_deferred_tramp<W, T, F, R, const LOW: bool>(w: &W)
where
    W: WrapperWrapped<T> + MaxObj,
    T: 'static,
    F: Fn(&T) -> R + Copy + Send + 'static,
    R: MethodResult,
{
    // SAFETY: `F` is a zero-sized `Copy` callable, see `zst`
    defer_or_run::<W, T, R, _, LOW>(w, "bang", |t: &T| (unsafe { zst::<F>() })(t));
}

extern "C" fn int_deferred_tramp<W, T, F, R, const LOW: bool>(w: &W, v: max_sys::t_atom_long)
where
    W: WrapperWrapped<T> + MaxObj,
    T: 'static,
    F: Fn(&T, max_sys::t_atom_long) -> R + Copy + Send + 'static,
    R: MethodResult,
{
    // SAFETY: `F` is a zero-sized `Copy` callable, see `zst`
    defer_or_run::<W, T, R, _, LOW>(w, "int", move |t: &T| (unsafe { zst::<F>() })(t, v));
}

extern "C" fn float_deferred_tramp<W, T, F, R, const LOW: bool>(w: &W, v: f64)
where
    W: WrapperWrapped<T> + MaxObj,
    T: 'static,
    F: Fn(&T, f64) -> R + Copy + Send + 'static,
    R: MethodResult,
{
    // SAFETY: `F` is a zero-sized `Copy` callable, see `zst`
    defer_or_run::<W, T, R, _, LOW>(w, "float", move |t: &T| (unsafe { zst::<F>() })(t, v));
}

extern "C" fn sym_deferred_tramp<W, T, F, R, const LOW: bool>(w: &W, s: *mut max_sys::t_symbol)
where
    W: WrapperWrapped<T> + MaxObj,
    T: 'static,
    F: Fn(&T, SymbolRef) -> R + Copy + Send + 'static,
    R: MethodResult,
{
    // SAFETY: Max passes a valid symbol (or null, the empty symbol) for an `A_SYM` argument
    let s = unsafe { SymbolRef::from_raw(s) };
    // SAFETY: `F` is a zero-sized `Copy` callable, see `zst`
    defer_or_run::<W, T, R, _, LOW>(w, "symbol", move |t: &T| (unsafe { zst::<F>() })(t, s));
}

extern "C" fn list_deferred_tramp<W, T, F, R, const LOW: bool>(
    w: &W,
    _sel: *mut max_sys::t_symbol,
    ac: c_long,
    av: *const max_sys::t_atom,
) where
    W: WrapperWrapped<T> + MaxObj,
    T: 'static,
    F: Fn(&T, &[Atom]) -> R + Copy + Send + 'static,
    R: MethodResult,
{
    // SAFETY: Max passes `ac` valid atoms that live for the call for an `A_GIMME` method; they
    // are copied
    let atoms = OwnedAtoms::copy(unsafe { atoms(ac, av) });
    // SAFETY: `F` is a zero-sized `Copy` callable, see `zst`
    defer_or_run::<W, T, R, _, LOW>(w, "list", move |t: &T| {
        (unsafe { zst::<F>() })(t, atoms.as_slice())
    });
}

/// `anything` and `A_GIMME` selector methods: the selector and the atoms.
extern "C" fn gimme_deferred_tramp<W, T, F, R, A, const LOW: bool, const ANY: bool>(
    w: &W,
    sel: *mut max_sys::t_symbol,
    ac: c_long,
    av: *const max_sys::t_atom,
) where
    W: WrapperWrapped<T> + MaxObj,
    T: 'static,
    F: GimmeFn<T, R, A>,
    R: MethodResult,
{
    // SAFETY: Max passes a valid selector and `ac` valid atoms that live for the call for an
    // `A_GIMME` method; they are copied
    let (sel, atoms) = unsafe { (SymbolRef::from_raw(sel), OwnedAtoms::copy(atoms(ac, av))) };
    let what = if ANY { "anything" } else { "method" };
    // SAFETY: `F` is a zero-sized `Copy` callable, see `zst`
    defer_or_run::<W, T, R, _, LOW>(w, what, move |t: &T| {
        F::call(unsafe { zst::<F>() }, t, &sel, atoms.as_slice())
    });
}

/// Pick the `LOW` instance of a deferring trampoline.
macro_rules! deferred {
    ($mode:expr, $ty:ty, $tramp:ident::<$($p:ty),*>) => {{
        let p: $ty = match $mode {
            Defer::Low => $tramp::<$($p,)* true>,
            Defer::Front => $tramp::<$($p,)* false>,
        };
        p
    }};
    ($mode:expr, $ty:ty, $tramp:ident::<$($p:ty),*>, $any:literal) => {{
        let p: $ty = match $mode {
            Defer::Low => $tramp::<$($p,)* true, $any>,
            Defer::Front => $tramp::<$($p,)* false, $any>,
        };
        p
    }};
}

pub(crate) fn bang_deferred_method<W, T, F, R>(_f: F, mode: Defer) -> MaxMethod
where
    W: WrapperWrapped<T> + MaxObj,
    T: 'static,
    F: Fn(&T) -> R + Copy + Send + 'static,
    R: MethodResult,
{
    let p = deferred!(mode, extern "C" fn(&W), bang_deferred_tramp::<W, T, F, R>);
    // SAFETY: the trampoline takes the object, as registered for `bang`
    unsafe { erase(p) }
}

pub(crate) fn int_deferred_method<W, T, F, R>(_f: F, mode: Defer) -> MaxMethod
where
    W: WrapperWrapped<T> + MaxObj,
    T: 'static,
    F: Fn(&T, max_sys::t_atom_long) -> R + Copy + Send + 'static,
    R: MethodResult,
{
    let p = deferred!(
        mode,
        extern "C" fn(&W, max_sys::t_atom_long),
        int_deferred_tramp::<W, T, F, R>
    );
    // SAFETY: the trampoline takes the object and a `t_atom_long`, as registered for `int`
    unsafe { erase(p) }
}

pub(crate) fn float_deferred_method<W, T, F, R>(_f: F, mode: Defer) -> MaxMethod
where
    W: WrapperWrapped<T> + MaxObj,
    T: 'static,
    F: Fn(&T, f64) -> R + Copy + Send + 'static,
    R: MethodResult,
{
    let p = deferred!(
        mode,
        extern "C" fn(&W, f64),
        float_deferred_tramp::<W, T, F, R>
    );
    // SAFETY: the trampoline takes the object and a double, as registered for `float`
    unsafe { erase(p) }
}

pub(crate) fn sym_deferred_method<W, T, F, R>(_f: F, mode: Defer) -> MaxMethod
where
    W: WrapperWrapped<T> + MaxObj,
    T: 'static,
    F: Fn(&T, SymbolRef) -> R + Copy + Send + 'static,
    R: MethodResult,
{
    let p = deferred!(
        mode,
        extern "C" fn(&W, *mut max_sys::t_symbol),
        sym_deferred_tramp::<W, T, F, R>
    );
    // SAFETY: the trampoline takes the object and a symbol, as registered for `symbol`
    unsafe { erase(p) }
}

pub(crate) fn list_deferred_method<W, T, F, R>(_f: F, mode: Defer) -> MaxMethod
where
    W: WrapperWrapped<T> + MaxObj,
    T: 'static,
    F: Fn(&T, &[Atom]) -> R + Copy + Send + 'static,
    R: MethodResult,
{
    let p = deferred!(mode, GimmeTramp<W>, list_deferred_tramp::<W, T, F, R>);
    // SAFETY: as `list_method`
    unsafe { erase(p) }
}

pub(crate) fn any_deferred_method<W, T, F, R>(_f: F, mode: Defer) -> MaxMethod
where
    W: WrapperWrapped<T> + MaxObj,
    T: 'static,
    F: Fn(&T, &SymbolRef, &[Atom]) -> R + Copy + Send + Sync + 'static,
    R: MethodResult,
{
    let p = deferred!(
        mode,
        GimmeTramp<W>,
        gimme_deferred_tramp::<W, T, F, R, WithSel>,
        true
    );
    // SAFETY: as `list_method`
    unsafe { erase(p) }
}

pub(crate) fn sel_gimme_deferred_method<W, T, F, R, A>(_f: F, mode: Defer) -> MaxMethod
where
    W: WrapperWrapped<T> + MaxObj,
    T: 'static,
    F: GimmeFn<T, R, A>,
    R: MethodResult,
{
    let p = deferred!(
        mode,
        GimmeTramp<W>,
        gimme_deferred_tramp::<W, T, F, R, A>,
        false
    );
    // SAFETY: as `list_method`
    unsafe { erase(p) }
}
