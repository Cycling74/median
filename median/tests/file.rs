//! `median::file::FilePath` against the `median-mock` file functions: locating, reading text,
//! closing the handles and the cancelled dialog.
//!
//! Every test takes `median_mock::lock()` for its whole body.

use median::{
    file::{FilePath, FileReadError, LocateError, TextLineBreak, fourcc},
    symbol::SymbolRef,
};
use median_mock::file::{add_file, open_file_count};

fn text_type() -> max_sys::t_fourcc {
    fourcc(['T', 'E', 'X', 't'])
}

fn sym(s: &str) -> SymbolRef {
    SymbolRef::try_from(s).unwrap()
}

#[test]
fn locates_a_registered_file_and_fails_cleanly_for_others() {
    let _k = median_mock::lock();
    add_file("notes.txt", 7, text_type(), b"hello");

    let found = FilePath::locate(&sym("notes.txt"), None).ok().unwrap();
    assert_eq!(found.file_name.to_str().unwrap(), "notes.txt");
    assert_eq!(found.vol, 7);
    assert_eq!(found.typ, text_type());

    // the type filter applies
    let text = vec![text_type()];
    assert!(FilePath::locate(&sym("notes.txt"), Some(&text)).is_ok());
    let other = vec![fourcc(['J', 'S', 'O', 'N'])];
    assert!(matches!(
        FilePath::locate(&sym("notes.txt"), Some(&other)),
        Err(LocateError::NotFound)
    ));

    assert!(matches!(
        FilePath::locate(&sym("missing.txt"), None),
        Err(LocateError::NotFound)
    ));
    assert!(matches!(
        FilePath::locate(&SymbolRef::default(), None),
        Err(LocateError::NameEmpty)
    ));
    let long = "x".repeat(4096);
    assert!(matches!(
        FilePath::locate(&sym(&long), None),
        Err(LocateError::NameTooLong)
    ));
    assert_eq!(open_file_count(), 0);
}

#[test]
fn reads_text_with_line_breaks_translated_and_closes_the_file() {
    let _k = median_mock::lock();
    add_file("lines.txt", 1, text_type(), b"one\r\ntwo\rthree\nfour");
    let path = FilePath::locate(&sym("lines.txt"), None).ok().unwrap();

    let unix = path.read_text(TextLineBreak::Unix, None).ok().unwrap();
    assert_eq!(unix.to_str().unwrap(), "one\ntwo\nthree\nfour");
    assert_eq!(open_file_count(), 0);

    let windows = path.read_text(TextLineBreak::Windows, None).ok().unwrap();
    assert_eq!(windows.to_str().unwrap(), "one\r\ntwo\r\nthree\r\nfour");
    assert_eq!(open_file_count(), 0);

    let mac = path.read_text(TextLineBreak::Mac, None).ok().unwrap();
    assert_eq!(mac.to_str().unwrap(), "one\rtwo\rthree\rfour");
    assert_eq!(open_file_count(), 0);

    // a maximum length of `Some(0)` reads everything, a smaller one stops early
    let all = path.read_text(TextLineBreak::Unix, Some(0)).ok().unwrap();
    assert_eq!(all, unix);
    let part = path.read_text(TextLineBreak::Unix, Some(3)).ok().unwrap();
    assert_eq!(part.to_str().unwrap(), "one");
    assert_eq!(open_file_count(), 0);
}

#[test]
fn an_empty_file_reads_as_empty_text() {
    let _k = median_mock::lock();
    add_file("empty.txt", 1, text_type(), b"");
    let path = FilePath::locate(&sym("empty.txt"), None).ok().unwrap();
    let text = path.read_text(TextLineBreak::Unix, None).ok().unwrap();
    assert!(text.as_bytes().is_empty());
    assert_eq!(open_file_count(), 0);
}

#[test]
fn reading_a_file_that_cannot_be_opened_is_an_error_without_leaks() {
    let _k = median_mock::lock();
    add_file("there.txt", 1, text_type(), b"x");
    // right name, wrong volume
    let path = FilePath {
        file_name: c"there.txt".to_owned(),
        vol: 2,
        typ: text_type(),
    };
    assert!(matches!(
        path.read_text(TextLineBreak::Unix, None),
        Err(FileReadError::ErrorOpening)
    ));
    let path = FilePath {
        file_name: c"gone.txt".to_owned(),
        vol: 1,
        typ: text_type(),
    };
    assert!(matches!(
        path.read_text(TextLineBreak::Unix, None),
        Err(FileReadError::ErrorOpening)
    ));
    assert_eq!(open_file_count(), 0);
}

#[test]
fn the_dialog_is_cancelled() {
    let _k = median_mock::lock();
    add_file("notes.txt", 1, text_type(), b"hello");
    assert!(FilePath::open_dialog(None).is_none());
    assert!(FilePath::open_dialog(Some(&vec![text_type()])).is_none());
    // an empty name asks for the dialog, which reports the cancellation as `None`
    assert!(FilePath::find_with_dialog(&SymbolRef::default(), None).is_none());
    // a name is located, not asked for
    assert!(FilePath::find_with_dialog(&sym("notes.txt"), None).is_some());
    assert!(FilePath::find_with_dialog(&sym("missing.txt"), None).is_none());
    assert_eq!(open_file_count(), 0);
}

#[test]
fn reset_forgets_files() {
    {
        let _k = median_mock::lock();
        add_file("once.txt", 1, text_type(), b"x");
        assert!(FilePath::locate(&sym("once.txt"), None).is_ok());
    }
    let _k = median_mock::lock();
    assert!(FilePath::locate(&sym("once.txt"), None).is_err());
}
