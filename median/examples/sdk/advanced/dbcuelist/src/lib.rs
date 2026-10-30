//! A port of the SDK's `advanced/dbcuelist`: a cue list in an in-memory SQLite database that all
//! objects with the same `name` share; `cue.do` outputs each of a cue's events.
//!
//! Every database call is raw `max_sys` (`ext_database.h` has no median API). The C formats
//! names and values into its SQL inside double quotes, which SQLite reads as identifiers when they
//! match a column (`cue.destroy name` deletes every cue) and which a `"` in a value breaks out of;
//! the port writes them as single-quoted SQL string literals with `'` doubled. The C leaks the
//! `cue.do` query result; the port frees every result. Without a database (no `name` yet) a
//! message posts its error without querying. median-mock has no `db_*` functions, so only an
//! object without a name is tested; the queries, `cue.do` output and `atom_setparse` are untested.

use std::{
    ffi::{CStr, CString, c_long},
    sync::Mutex,
};

use median::{max_sys, prelude::*};

struct State {
    name: SymbolRef,
    // Null until `name` is set.
    db: *mut max_sys::t_database,
}

// SAFETY: the database handle is used only with the `Mutex` held; the C uses it from whichever
// thread a message arrives on.
unsafe impl Send for State {}

struct QueryResult(*mut max_sys::t_db_result);

impl QueryResult {
    fn strings(&self) -> Vec<String> {
        // SAFETY: `self.0` is a live result from `db_query_direct`; the strings it returns stay
        // valid while it does and are copied before then.
        unsafe {
            (0..max_sys::db_result_numrecords(self.0))
                .filter_map(|i| {
                    let s = max_sys::db_result_string(self.0, i, 0);
                    (!s.is_null()).then(|| CStr::from_ptr(s).to_string_lossy().into_owned())
                })
                .collect()
        }
    }
}

impl Drop for QueryResult {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: a result from `db_query_direct` is the caller's to free with `object_free`.
            unsafe { max_sys::object_free(self.0.cast()) };
        }
    }
}

#[derive(MaxObj)]
#[max(name = "median.dbcuelist")]
pub struct Dbcuelist {
    obj: ObjRef,
    state: Mutex<State>,
    out: Outlet<Any>,
}

impl MaxObjWrapped for Dbcuelist {
    fn new(builder: &mut MaxBuilder<Self>) -> Result<Self, NewError> {
        Ok(Self {
            obj: builder.obj(),
            state: Mutex::new(State {
                name: SymbolRef::default(),
                db: std::ptr::null_mut(),
            }),
            out: builder.outlet(Outlet::any()),
        })
    }
}

impl Drop for Dbcuelist {
    fn drop(&mut self) {
        let state = self.state.get_mut().unwrap();
        if !state.db.is_null() {
            // SAFETY: `db` was opened by `db_open` and is closed only here or when `name` changes.
            unsafe { max_sys::db_close(&mut state.db) };
        }
    }
}

#[median::methods]
impl Dbcuelist {
    #[sel("cue.create")]
    pub fn cue_create(&self, s: SymbolRef) {
        let sql = format!("INSERT INTO cues ( name ) VALUES ( {} )", literal(&s));
        // The C's message here is "error creating event".
        self.run(&sql, "error creating cue");
    }

    #[sel("cue.destroy")]
    pub fn cue_destroy(&self, s: SymbolRef) {
        let sql = format!("DELETE FROM cues WHERE name = {}", literal(&s));
        self.run(&sql, "error deleting cue");
    }

    #[sel("cue.rename")]
    pub fn cue_rename(&self, from: SymbolRef, to: SymbolRef) {
        let sql = format!(
            "UPDATE cues SET name = {} WHERE name = {}",
            literal(&to),
            literal(&from)
        );
        // The C's message here is "error deleting cue".
        self.run(&sql, "error renaming cue");
    }

    #[sel("cue.do")]
    pub fn cue_do(&self, s: SymbolRef) {
        let sql = format!(
            "SELECT value FROM events WHERE cue_id_ext = \
             (SELECT cue_id FROM cues WHERE name = {})",
            literal(&s)
        );
        let Some(result) = self.query(&sql) else {
            self.obj.error("invalid cue specified for 'do'");
            return;
        };
        let event = median::sym!("event");
        for text in result.strings() {
            self.output_parsed(&event, &text);
        }
    }

    #[gimme("event.create")]
    pub fn event_create(&self, atoms: &[Atom]) {
        if atoms.len() < 2 {
            self.obj
                .error("at least two arguments required to create an event");
            return;
        }
        let (first, values) = (&atoms[0], &atoms[1..]);
        let text = values.iter().map(Atom::text).collect::<Vec<_>>().join(" ");
        let sql = format!(
            "INSERT INTO events ( cue_id_ext, value ) \
             VALUES ( (SELECT cue_id FROM cues WHERE name = {}), {} )",
            literal(&first.get_symbol()),
            quote(&text)
        );
        self.run(&sql, "error creating event");
    }

    #[sel("event.destroy")]
    pub fn event_destroy(&self, id: t_atom_long) {
        let sql = format!("DELETE FROM events WHERE event_id = {id}");
        self.run(&sql, "error deleting event");
    }

    #[attr]
    pub fn name(&self) -> SymbolRef {
        self.state.lock().unwrap().name
    }

    /// Open the database `name` (closing the old one) and make its tables.
    pub fn set_name(&self, name: SymbolRef) {
        let mut state = self.state.lock().unwrap();
        if name == state.name {
            return;
        }
        state.name = name;
        // SAFETY: `db` is null or open; `db_close` closes it and nulls it, `db_open` with a null
        // path opens (or shares) an in-memory database. The table names are NUL-terminated.
        // GAP: db_open, db_close, db_query_table_new, db_query_table_addcolumn
        unsafe {
            if !state.db.is_null() {
                max_sys::db_close(&mut state.db);
            }
            max_sys::db_open(name.as_ptr(), std::ptr::null(), &mut state.db);
            if state.db.is_null() {
                return;
            }
            // These fail when another object of the same name has made them already.
            let db = state.db;
            max_sys::db_query_table_new(db, c"cues".as_ptr());
            max_sys::db_query_table_addcolumn(
                db,
                c"cues".as_ptr(),
                c"name".as_ptr(),
                c"VARCHAR(256)".as_ptr(),
                std::ptr::null(),
            );
            max_sys::db_query_table_new(db, c"events".as_ptr());
            max_sys::db_query_table_addcolumn(
                db,
                c"events".as_ptr(),
                c"cue_id_ext".as_ptr(),
                c"INTEGER".as_ptr(),
                std::ptr::null(),
            );
            max_sys::db_query_table_addcolumn(
                db,
                c"events".as_ptr(),
                c"value".as_ptr(),
                c"TEXT".as_ptr(),
                std::ptr::null(),
            );
        }
    }

    fn run(&self, sql: &str, error: &str) {
        if self.query(sql).is_none() {
            self.obj.error(error);
        }
    }

    /// `None` if there is no database or the query fails.
    fn query(&self, sql: &str) -> Option<QueryResult> {
        let state = self.state.lock().unwrap();
        if state.db.is_null() {
            return None;
        }
        let sql = CString::new(sql).ok()?;
        let mut result = QueryResult(std::ptr::null_mut());
        // SAFETY: `db` is open while the lock is held, `sql` is NUL-terminated and has no format
        // codes interpreted (`db_query_direct` is not variadic), and `result.0` starts null so a
        // new result is made, which `QueryResult` frees.
        // GAP: db_query_direct
        let err = unsafe { max_sys::db_query_direct(state.db, &mut result.0, sql.as_ptr()) };
        (err == 0).then_some(result)
    }

    fn output_parsed(&self, sel: &SymbolRef, text: &str) {
        let Ok(text) = CString::new(text) else {
            return;
        };
        let mut ac: c_long = 0;
        let mut av: *mut max_sys::t_atom = std::ptr::null_mut();
        // SAFETY: `text` is NUL-terminated; on return `av` is null or `ac` atoms allocated for the
        // caller, which are read as `Atom`s (`#[repr(transparent)]` over `t_atom`) and then freed.
        // GAP: atom_setparse
        unsafe {
            max_sys::atom_setparse(&mut ac, &mut av, text.as_ptr());
            if av.is_null() {
                return;
            }
            if ac > 0 {
                let atoms = std::slice::from_raw_parts(av.cast::<Atom>(), ac as usize);
                let _ = self.out.send(sel, atoms);
            }
            max_sys::sysmem_freeptr(av.cast());
        }
    }
}

/// `s` as an SQL string literal.
fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

fn literal(s: &SymbolRef) -> String {
    quote(&s.name().to_string_lossy())
}

median::ext_main! { Dbcuelist }

#[cfg(test)]
mod tests {
    use median_mock::Atom;

    #[test]
    fn messages_fail_without_a_database() {
        let k = median_mock::lock();
        // SAFETY: registration on the mock's main thread, as Max calls it.
        unsafe { super::ext_main(std::ptr::null_mut()) };
        let x = k.new_object("median.dbcuelist", &[]);
        assert!(!x.is_null());
        assert_eq!(k.attr_get(x, "name").unwrap(), [Atom::Sym(String::new())]);
        let cue = || Atom::Sym("intro".into());
        k.send(x, "cue.create", &[cue()]).unwrap();
        k.send(x, "cue.destroy", &[cue()]).unwrap();
        k.send(x, "cue.rename", &[cue(), Atom::Sym("outro".into())])
            .unwrap();
        k.send(x, "cue.do", &[cue()]).unwrap();
        k.send(x, "event.create", &[cue()]).unwrap();
        k.send(x, "event.create", &[cue(), Atom::Long(1), Atom::Float(0.5)])
            .unwrap();
        k.send(x, "event.destroy", &[Atom::Long(3)]).unwrap();
        let errors: Vec<_> = k.take_console_log().into_iter().map(|e| e.text).collect();
        assert_eq!(
            errors,
            [
                "error creating cue",
                "error deleting cue",
                "error renaming cue",
                "invalid cue specified for 'do'",
                "at least two arguments required to create an event",
                "error creating event",
                "error deleting event",
            ]
        );
        assert!(k.take_outlet_log().is_empty());
        assert!(k.free_object(x));
    }

    #[test]
    fn quotes_sql_strings() {
        assert_eq!(super::quote("it's"), "'it''s'");
        assert_eq!(super::quote("a \"b\""), "'a \"b\"'");
    }
}
