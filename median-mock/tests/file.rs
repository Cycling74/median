//! The file API model: registered in-memory files, a cancelled dialog, text reads, closing.
use max_sys as sys;
use std::ffi::{CStr, c_char, c_short};

const TEXT: u32 = u32::from_be_bytes(*b"TEXT");
const JSON: u32 = u32::from_be_bytes(*b"JSON");
const READ: c_short = sys::e_max_openfile_permissions::PATH_READ_PERM as c_short;
const LB_UNIX: u32 = sys::t_sysfile_text_flags::TEXT_LB_UNIX;
const LB_PC: u32 = sys::t_sysfile_text_flags::TEXT_LB_PC;
const NUL: u32 = sys::t_sysfile_text_flags::TEXT_NULL_TERMINATE;

fn locate(name: &CStr, types: &[u32]) -> Option<(c_short, u32)> {
    let mut buf = [0 as c_char; 64];
    for (b, n) in buf.iter_mut().zip(name.to_bytes()) {
        *b = *n as c_char;
    }
    let (mut vol, mut typ) = (0 as c_short, 0u32);
    let r = unsafe {
        sys::locatefile_extended(
            buf.as_mut_ptr(),
            &mut vol,
            &mut typ,
            types.as_ptr(),
            types.len() as c_short,
        )
    };
    (r == 0).then_some((vol, typ))
}

fn read(name: &CStr, vol: c_short, maxlen: usize, flags: u32) -> Option<Vec<u8>> {
    unsafe {
        let mut fh = std::ptr::null_mut();
        if sys::path_opensysfile(name.as_ptr(), vol, &mut fh, READ) != 0 {
            return None;
        }
        let h = sys::sysmem_newhandle(0);
        let r = sys::sysfile_readtextfile(fh, h, maxlen, flags);
        let out = (r == 0).then(|| {
            let n = sys::sysmem_ptrsize((*h).cast());
            std::slice::from_raw_parts((*h).cast::<u8>(), n).to_vec()
        });
        sys::sysmem_freehandle(h);
        assert_eq!(sys::sysfile_close(fh), 0);
        out
    }
}

#[test]
fn finds_only_registered_files_and_filters_by_type() {
    let _k = median_mock::lock();
    assert_eq!(locate(c"a.txt", &[]), None);
    median_mock::file::add_file("a.txt", 7, TEXT, b"hi");
    assert_eq!(locate(c"a.txt", &[]), Some((7, TEXT)));
    assert_eq!(locate(c"a.txt", &[TEXT, JSON]), Some((7, TEXT)));
    assert_eq!(locate(c"a.txt", &[JSON]), None);
    assert_eq!(locate(c"b.txt", &[]), None);
}

#[test]
fn dialog_is_cancelled() {
    let _k = median_mock::lock();
    let mut name = [0 as c_char; 8];
    let (mut vol, mut typ) = (0 as c_short, 0u32);
    let r = unsafe {
        sys::open_dialog(
            name.as_mut_ptr(),
            &mut vol,
            &mut typ,
            std::ptr::null_mut(),
            0,
        )
    };
    assert_ne!(r, 0);
    assert_eq!((name[0], vol, typ), (0, 0, 0));
}

#[test]
fn reads_text_with_line_break_translation_and_closes() {
    let _k = median_mock::lock();
    median_mock::file::add_file("t.txt", 3, TEXT, b"one\r\ntwo\rthree\nfour");
    assert_eq!(
        read(c"t.txt", 3, 0, LB_UNIX | NUL).unwrap(),
        b"one\ntwo\nthree\nfour\0"
    );
    assert_eq!(
        read(c"t.txt", 3, 0, LB_PC).unwrap(),
        b"one\r\ntwo\r\nthree\r\nfour"
    );
    assert_eq!(read(c"t.txt", 3, 3, NUL).unwrap(), b"one\0");
    // no line-break flag: the bytes come back as they are
    assert_eq!(
        read(c"t.txt", 3, 0, NUL).unwrap(),
        b"one\r\ntwo\rthree\nfour\0"
    );
    assert_eq!(median_mock::file::open_file_count(), 0);
    //wrong volume, missing file
    assert_eq!(read(c"t.txt", 4, 0, NUL), None);
    assert_eq!(read(c"x.txt", 3, 0, NUL), None);
    //an empty file reads as an empty text
    median_mock::file::add_file("e.txt", 3, TEXT, b"");
    assert_eq!(read(c"e.txt", 3, 0, 0).unwrap(), b"");
}

#[test]
fn handles_are_tracked_and_write_access_is_refused() {
    let k = median_mock::lock();
    median_mock::file::add_file("t.txt", 3, TEXT, b"x");
    unsafe {
        let mut fh = std::ptr::null_mut();
        let write = sys::e_max_openfile_permissions::PATH_WRITE_PERM as c_short;
        assert_ne!(
            sys::path_opensysfile(c"t.txt".as_ptr(), 3, &mut fh, write),
            0
        );
        assert_eq!(
            sys::path_opensysfile(c"t.txt".as_ptr(), 3, &mut fh, READ),
            0
        );
        assert_eq!(median_mock::file::open_file_count(), 1);
        assert_eq!(sys::sysfile_close(fh), 0);
        assert_eq!(median_mock::file::open_file_count(), 0);
        //closing twice is an error, not a double free
        assert_ne!(sys::sysfile_close(fh), 0);
        //a file left open is released by the next reset
        assert_eq!(
            sys::path_opensysfile(c"t.txt".as_ptr(), 3, &mut fh, READ),
            0
        );
    }
    k.reset();
    assert_eq!(median_mock::file::open_file_count(), 0);
    assert_eq!(locate(c"t.txt", &[]), None);
}
