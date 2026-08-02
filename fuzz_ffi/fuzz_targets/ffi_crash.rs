//! Crash-only FFI fuzz harness (no C oracle).
//!
//! Links the Rust port with `full-suite-abi`. Opcode bytes drive init / write /
//! read / expect / node / destroy calls on stack buffers. Unwinding panics fail
//! the target; sticky ABI errors are expected and ignored.
//!
//! Fixed-buffer writer only (growable uses suite `test_free` noop under cargo
//! test shims and would leak across libFuzzer iterations).

#![no_main]

use std::ffi::c_char;
use std::mem::MaybeUninit;
use std::panic::{catch_unwind, AssertUnwindSafe};

use libfuzzer_sys::fuzz_target;
use mpack::ffi::types::{MpackNode, MpackReader, MpackTree, MpackWriter};
use mpack::ffi::writer::{
    mpack_complete_array, mpack_complete_map, mpack_start_array, mpack_start_map, mpack_write_bool,
    mpack_write_i64, mpack_write_nil, mpack_write_str, mpack_write_u64, mpack_writer_destroy,
    mpack_writer_init,
};

extern "C" {
    fn mpack_reader_init_data(reader: *mut MpackReader, data: *const c_char, count: usize);
    fn mpack_reader_destroy(reader: *mut MpackReader) -> i32;
    fn mpack_read_tag(reader: *mut MpackReader) -> mpack::ffi::types::MpackTag;
    fn mpack_discard(reader: *mut MpackReader);
    fn mpack_expect_nil(reader: *mut MpackReader);
    fn mpack_expect_u64(reader: *mut MpackReader) -> u64;
    fn mpack_expect_bool(reader: *mut MpackReader) -> bool;
    fn mpack_tree_init_data(tree: *mut MpackTree, data: *const c_char, length: usize);
    fn mpack_tree_parse(tree: *mut MpackTree);
    fn mpack_tree_destroy(tree: *mut MpackTree) -> i32;
    fn mpack_tree_root(tree: *mut MpackTree) -> MpackNode;
    fn mpack_node_nil(node: MpackNode);
    fn mpack_node_u64(node: MpackNode) -> u64;
}

const MAX_INPUT: usize = 65536;
const OP_COUNT: u8 = 10;

struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn u8(&mut self) -> u8 {
        if self.pos < self.data.len() {
            let v = self.data[self.pos];
            self.pos += 1;
            v
        } else {
            0
        }
    }

    fn u64(&mut self) -> u64 {
        let mut bytes = [0u8; 8];
        for b in &mut bytes {
            *b = self.u8();
        }
        u64::from_le_bytes(bytes)
    }

    fn slice(&mut self, n: usize) -> &'a [u8] {
        let start = self.pos.min(self.data.len());
        let end = (start + n).min(self.data.len());
        self.pos = end;
        &self.data[start..end]
    }

    fn remaining(&self) -> bool {
        self.pos < self.data.len()
    }
}

fn run_ops(data: &[u8]) {
    let mut cursor = Cursor::new(data);
    let mut buf = [0u8; 512];
    let mut writer = MaybeUninit::<MpackWriter>::uninit();
    let mut reader = MaybeUninit::<MpackReader>::uninit();
    let mut tree = MaybeUninit::<MpackTree>::uninit();
    let mut writer_live = false;
    let mut reader_live = false;
    let mut tree_live = false;

    while cursor.remaining() {
        let op = cursor.u8() % OP_COUNT;
        unsafe {
            match op {
                0 => {
                    if writer_live {
                        let _ = mpack_writer_destroy(writer.as_mut_ptr());
                    }
                    mpack_writer_init(
                        writer.as_mut_ptr(),
                        buf.as_mut_ptr().cast(),
                        buf.len(),
                    );
                    writer_live = true;
                }
                1 => {
                    if writer_live {
                        mpack_write_nil(writer.as_mut_ptr());
                    }
                }
                2 => {
                    if writer_live {
                        mpack_write_bool(writer.as_mut_ptr(), cursor.u8() & 1 != 0);
                    }
                }
                3 => {
                    if writer_live {
                        mpack_write_u64(writer.as_mut_ptr(), cursor.u64());
                    }
                }
                4 => {
                    if writer_live {
                        let n = (cursor.u8() as usize) % 32;
                        let bytes = cursor.slice(n);
                        mpack_write_str(
                            writer.as_mut_ptr(),
                            bytes.as_ptr().cast(),
                            bytes.len() as u32,
                        );
                    }
                }
                5 => {
                    if writer_live {
                        mpack_start_array(writer.as_mut_ptr(), (cursor.u8() % 4) as u32);
                        mpack_write_nil(writer.as_mut_ptr());
                        mpack_complete_array(writer.as_mut_ptr());
                    }
                }
                6 => {
                    if writer_live {
                        mpack_start_map(writer.as_mut_ptr(), 1);
                        mpack_write_u64(writer.as_mut_ptr(), 1);
                        mpack_write_i64(writer.as_mut_ptr(), -1);
                        mpack_complete_map(writer.as_mut_ptr());
                    }
                }
                7 => {
                    let n = (cursor.u8() as usize) % 64;
                    let payload = cursor.slice(n);
                    if reader_live {
                        let _ = mpack_reader_destroy(reader.as_mut_ptr());
                    }
                    mpack_reader_init_data(
                        reader.as_mut_ptr(),
                        payload.as_ptr().cast(),
                        payload.len(),
                    );
                    reader_live = true;
                    let _ = mpack_read_tag(reader.as_mut_ptr());
                    mpack_discard(reader.as_mut_ptr());
                    mpack_expect_nil(reader.as_mut_ptr());
                    let _ = mpack_expect_u64(reader.as_mut_ptr());
                    let _ = mpack_expect_bool(reader.as_mut_ptr());
                }
                8 => {
                    let n = (cursor.u8() as usize) % 128;
                    let payload = cursor.slice(n);
                    if tree_live {
                        let _ = mpack_tree_destroy(tree.as_mut_ptr());
                    }
                    mpack_tree_init_data(
                        tree.as_mut_ptr(),
                        payload.as_ptr().cast(),
                        payload.len(),
                    );
                    tree_live = true;
                    mpack_tree_parse(tree.as_mut_ptr());
                    let root = mpack_tree_root(tree.as_mut_ptr());
                    mpack_node_nil(root);
                    let _ = mpack_node_u64(root);
                }
                9 => {
                    if writer_live {
                        let _ = mpack_writer_destroy(writer.as_mut_ptr());
                        writer_live = false;
                    }
                    if reader_live {
                        let _ = mpack_reader_destroy(reader.as_mut_ptr());
                        reader_live = false;
                    }
                    if tree_live {
                        let _ = mpack_tree_destroy(tree.as_mut_ptr());
                        tree_live = false;
                    }
                }
                _ => {}
            }
        }
    }

    unsafe {
        if writer_live {
            let _ = mpack_writer_destroy(writer.as_mut_ptr());
        }
        if reader_live {
            let _ = mpack_reader_destroy(reader.as_mut_ptr());
        }
        if tree_live {
            let _ = mpack_tree_destroy(tree.as_mut_ptr());
        }
    }
}

fuzz_target!(|data: &[u8]| {
    let data = if data.len() > MAX_INPUT {
        &data[..MAX_INPUT]
    } else {
        data
    };
    let result = catch_unwind(AssertUnwindSafe(|| run_ops(data)));
    if result.is_err() {
        panic!("ffi_crash: FFI path unwound");
    }
});
