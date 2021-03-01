//! Methods and types that manage threading and execution.

/// A signature for a method that can be defered.
pub type DeferMethod = unsafe extern "C" fn(
    obj: *mut max_sys::t_object,
    sym: *mut max_sys::t_symbol,
    argc: std::os::raw::c_long,
    argv: *const max_sys::t_atom,
);

/// Defer execution of a method to the main thread if (and only if) you are calling from the
/// scheduler thread.
///
/// # Arguments
///	* `method` - The method to be called.
///	* `obj` - First argument passed to the method when it executes.
///	* `sym` - The symbol to pass method when it executes.
/// * `args` - Additional args to pass to the method when it executes. Will make a copy.
///
/// # Remarks
/// This function uses the isr() routine to determine whether you're at the Max timer interrupt
/// level (in the scheduler thread). If so, defer() creates a Qelem (see Qelems), calls
/// qelem_front(), and its queue function calls the function fn you passed with the specified
/// arguments. If you're not in the scheduler thread, the function is executed immediately with the
/// arguments. Note that this implies that defer() is not appropriate for using in situations such
/// as Device or File manager i/o completion routines. The defer_low() function is appropriate
/// however, because it always defers.
pub fn defer(
    method: DeferMethod,
    obj: *mut max_sys::t_object,
    sym: crate::symbol::SymbolRef,
    args: &[crate::atom::Atom],
) {
    unsafe {
        let _ = max_sys::defer(
            obj as _,
            Some(std::mem::transmute::<_, _>(method)),
            sym.inner(),
            args.len() as _,
            std::mem::transmute::<_, _>(args.as_ptr()), //should have been const in max_sys
        );
    }
}

/// Defer execution of a function to the back of the queue on the main thread.
///
/// # Arguments
///	* `method` - The method to be called.
///	* `obj` - First argument passed to the method when it executes.
///	* `sym` - The symbol to pass method when it executes.
/// * `args` - Additional args to pass to the method when it executes. Will make a copy.
///
/// # Remarks
/// Always defers a call to the function fun whether you are already in the main thread or not, and
/// uses qelem_set(), not qelem_front(). This function is recommended for responding to messages
/// that will cause your object to open a dialog box, such as read and write.
pub fn defer_low(
    method: DeferMethod,
    obj: *mut max_sys::t_object,
    sym: crate::symbol::SymbolRef,
    args: &[crate::atom::Atom],
) {
    unsafe {
        let _ = max_sys::defer_low(
            obj as _,
            Some(std::mem::transmute::<_, _>(method)),
            sym.inner(),
            args.len() as _,
            std::mem::transmute::<_, _>(args.as_ptr()), //should have been const in max_sys
        );
    }
}
