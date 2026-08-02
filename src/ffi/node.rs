//! C ABI boundary for the MPack tree/node parser (`full-suite-abi`).
//!
//! Architecture: C owns `mpack_tree_t` storage (inline ABI struct). Each tree
//! has a companion `FfiTreeState` in a global side-table keyed by raw pointer.
//! Parse calls `Tree::parse_with_limits` from the safe core, then materialises
//! ABI nodes into `Box<[MpackNodeData]>` (heap) or the user-provided pool.
//!
//! BFS ordering: children of each container are contiguous in the ABI node
//! array so C's `children + index` pointer arithmetic is correct.
//!
//! Ext types: C MPack stores exttype at `tree->data[offset - 1]` (one byte
//! before the ext payload). Our safe-core `payload_off` already points one
//! past the exttype byte, matching this convention.

use std::collections::HashMap;
use std::ffi::{c_char, c_int, c_uint, c_void, CStr};
use std::io::Write as IoWrite;
use std::ptr;
use std::slice;
use std::sync::{Mutex, OnceLock};

use crate::common::Tag;
use crate::ffi::guard::catch_ffi_panic;
use crate::ffi::types::{
    core_error_to_abi, MpackError, MpackNode, MpackNodeData, MpackTag, MpackTimestamp, MpackTree,
    MpackTreeParser, MpackTreeRead, MPACK_ERROR_BUG, MPACK_ERROR_DATA, MPACK_ERROR_EOF,
    MPACK_ERROR_INVALID, MPACK_ERROR_IO, MPACK_ERROR_MEMORY, MPACK_ERROR_TOO_BIG,
    MPACK_ERROR_TYPE, MPACK_OK,
};
use crate::node::{NodeData, Tree};
use crate::reader;

// ── type code constants ───────────────────────────────────────────────────────

const TYPE_MISSING: c_int = 0;
const TYPE_NIL: c_int = 1;
const TYPE_BOOL: c_int = 2;
const TYPE_INT: c_int = 3;
const TYPE_UINT: c_int = 4;
const TYPE_FLOAT: c_int = 5;
const TYPE_DOUBLE: c_int = 6;
const TYPE_STR: c_int = 7;
const TYPE_BIN: c_int = 8;
const TYPE_ARRAY: c_int = 9;
const TYPE_MAP: c_int = 10;
const TYPE_EXT: c_int = 11;

const PRINT_BYTE_COUNT: usize = 12;
const INITIAL_STREAM_CAPACITY: usize = 4096;
const SEEK_SET: c_int = 0;
const SEEK_END: c_int = 2;

// ── externs ───────────────────────────────────────────────────────────────────

unsafe extern "C" {
    fn test_malloc(size: usize) -> *mut c_void;
    fn test_free(pointer: *mut c_void);
    fn mpack_assert_fail(message: *const c_char);
    fn mpack_break_hit(message: *const c_char);
    fn fopen(filename: *const c_char, mode: *const c_char) -> *mut c_void;
    fn fread(data: *mut c_void, size: usize, count: usize, file: *mut c_void) -> usize;
    fn fclose(file: *mut c_void) -> c_int;
    fn ftell(file: *mut c_void) -> i64;
    fn fseek(file: *mut c_void, offset: i64, whence: c_int) -> c_int;
    fn fwrite(data: *const c_void, size: usize, count: usize, file: *mut c_void) -> usize;
}

// ── side-table state ──────────────────────────────────────────────────────────

/// Per-tree Rust state, keyed by `tree as usize` in the global map.
struct FfiTreeState {
    nodes: Vec<NodeData>,
    root: Option<usize>,
    size: usize,
    parsed: bool,
    /// ABI nodes on the heap; `*mut MpackNodeData` pointers in the tree point here.
    heap_nodes: Box<[MpackNodeData]>,
    using_pool: bool,
    /// Owned data for stream/file inits.
    owned_data: Vec<u8>,
    /// FILE* to fclose on destroy (if Some).
    close_file: Option<*mut c_void>,
    max_size: usize,
    max_nodes: usize,
}

// SAFETY: close_file is a C FILE* accessed only within the Mutex-guarded
// section by the single thread calling destroy.
unsafe impl Send for FfiTreeState {}
unsafe impl Sync for FfiTreeState {}

fn ffi_trees() -> &'static Mutex<HashMap<usize, FfiTreeState>> {
    static TREES: OnceLock<Mutex<HashMap<usize, FfiTreeState>>> = OnceLock::new();
    TREES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn register_state(tree: *mut MpackTree, state: FfiTreeState) {
    if let Ok(mut map) = ffi_trees().lock() {
        map.insert(tree as usize, state);
    }
}

fn remove_state(tree: *mut MpackTree) -> Option<FfiTreeState> {
    ffi_trees().lock().ok()?.remove(&(tree as usize))
}

fn has_state(tree: *mut MpackTree) -> bool {
    ffi_trees()
        .lock()
        .is_ok_and(|map| map.contains_key(&(tree as usize)))
}

fn with_state<T>(tree: *mut MpackTree, default: T, f: impl FnOnce(&mut FfiTreeState) -> T) -> T {
    let Ok(mut map) = ffi_trees().lock() else {
        return default;
    };
    let Some(state) = map.get_mut(&(tree as usize)) else {
        return default;
    };
    f(state)
}

// ── tree helpers ──────────────────────────────────────────────────────────────

fn break_hit(msg: &[u8]) {
    unsafe { mpack_break_hit(msg.as_ptr().cast()) };
}

fn assert_fail(msg: &[u8]) {
    unsafe { mpack_assert_fail(msg.as_ptr().cast()) };
}

/// Revoke all ABI parse views before dropping or replacing their side-table
/// owners. `t.root` must never outlive `heap_nodes`.
fn clear_abi_parse_views(tree: *mut MpackTree) {
    if tree.is_null() {
        return;
    }
    let t = unsafe { &mut *tree };
    t.root = ptr::addr_of_mut!(t.nil_node);
    t.size = 0;
    t.node_count = 0;
    t.parser = MpackTreeParser::empty();
    with_state(tree, (), |s| {
        s.nodes.clear();
        s.root = None;
        s.size = 0;
        s.parsed = false;
        s.heap_nodes = Box::new([]);
    });
}

/// Advances past a previously parsed message (multi-`parse` on one tree).
fn advance_previous_message(tree: *mut MpackTree) {
    let t = unsafe { &mut *tree };
    let size = t.size;
    if size == 0 {
        return;
    }
    // Revoke the heap-backed root before clearing its owner below.
    t.root = ptr::addr_of_mut!(t.nil_node);
    with_state(tree, (), |s| {
        if !s.owned_data.is_empty() {
            let drain = size.min(s.owned_data.len());
            s.owned_data.drain(..drain);
            t.data = s.owned_data.as_ptr().cast();
            t.data_length = s.owned_data.len();
        } else if !t.data.is_null() && t.data_length >= size {
            t.data = unsafe { t.data.add(size) };
            t.data_length -= size;
        }
        s.nodes.clear();
        s.root = None;
        s.size = 0;
        s.parsed = false;
        s.heap_nodes = Box::new([]);
    });
    t.size = 0;
    t.node_count = 0;
    t.parser = MpackTreeParser::empty();
}

fn flag_tree_error(tree: *mut MpackTree, error: MpackError) {
    if tree.is_null() || error == MPACK_OK {
        return;
    }
    let t = unsafe { &mut *tree };
    if t.error != MPACK_OK {
        return;
    }
    t.error = error;
    if let Some(error_fn) = t.error_fn {
        unsafe { error_fn(tree, error) };
    }
}

/// Replace a sticky error (stream incomplete remaps Invalid → IO / TOO_BIG).
/// C leaves incomplete parses unflagged, then the outer `mpack_tree_parse`
/// flags IO/invalid; our one-shot safe-core parse flags Invalid first.
fn replace_tree_error(tree: *mut MpackTree, error: MpackError) {
    if tree.is_null() || error == MPACK_OK {
        return;
    }
    let t = unsafe { &mut *tree };
    t.error = MPACK_OK;
    flag_tree_error(tree, error);
}

/// Core Invalid/Eof from a fixed-buffer parse of stream-owned bytes means the
/// message was incomplete after fills (C: `continue_parsing` returned false
/// with `mpack_ok`). Remap to TOO_BIG when the fill hit `max_size`, else IO.
fn remap_stream_incomplete(tree: *mut MpackTree, hit_max_size: bool) {
    let err = tree_error(tree);
    if err != MPACK_ERROR_INVALID && err != MPACK_ERROR_EOF {
        return;
    }
    if hit_max_size {
        replace_tree_error(tree, MPACK_ERROR_TOO_BIG);
    } else {
        replace_tree_error(tree, MPACK_ERROR_IO);
    }
}

fn nil_node_for(tree: *mut MpackTree) -> MpackNode {
    if tree.is_null() {
        return MpackNode::null();
    }
    MpackNode {
        data: unsafe { std::ptr::addr_of_mut!((*tree).nil_node) },
        tree,
    }
}

fn missing_node_for(tree: *mut MpackTree) -> MpackNode {
    if tree.is_null() {
        return MpackNode::null();
    }
    MpackNode {
        data: unsafe { std::ptr::addr_of_mut!((*tree).missing_node) },
        tree,
    }
}

fn tree_error(tree: *mut MpackTree) -> MpackError {
    if tree.is_null() {
        return MPACK_ERROR_BUG;
    }
    unsafe { (*tree).error }
}

// ── node helpers ──────────────────────────────────────────────────────────────

fn node_tree_error(node: MpackNode) -> MpackError {
    if node.tree.is_null() {
        return MPACK_ERROR_BUG;
    }
    unsafe { (*node.tree).error }
}

/// Returns `node.data->type_`, or TYPE_NIL for null pointers.
fn nd_type(node: MpackNode) -> c_int {
    if node.data.is_null() {
        return TYPE_NIL;
    }
    unsafe { (*node.data).type_ }
}

/// Returns `node.data->len`, or 0 for null.
fn nd_len(node: MpackNode) -> u32 {
    if node.data.is_null() {
        return 0;
    }
    unsafe { (*node.data).len }
}

/// Returns `node.data->value`, or 0 for null.
fn nd_value(node: MpackNode) -> u64 {
    if node.data.is_null() {
        return 0;
    }
    unsafe { (*node.data).value }
}

/// Pointer to `tree->data` start.
fn tree_data_ptr(node: MpackNode) -> *const u8 {
    if node.tree.is_null() {
        return ptr::null();
    }
    unsafe { (*node.tree).data.cast::<u8>() }
}

// ── materialization ───────────────────────────────────────────────────────────

/// Writes ABI nodes into `dest` using BFS ordering so that each container's
/// children occupy a contiguous block. Returns the ABI index of `root` (always 0).
///
/// # Safety
/// `dest` must be writable for at least `safe_nodes.len()` elements.
unsafe fn materialize_nodes(
    safe_nodes: &[NodeData],
    root: usize,
    dest: *mut MpackNodeData,
) {
    let n = safe_nodes.len();
    if n == 0 {
        return;
    }

    // BFS assignment of ABI indices so children of every container are contiguous
    let mut abi_index = vec![0usize; n];
    let mut children_start_abi = vec![0usize; n];

    let mut queue = std::collections::VecDeque::new();
    abi_index[root] = 0;
    queue.push_back(root);
    let mut next_idx = 1usize;

    while let Some(safe_idx) = queue.pop_front() {
        let node = &safe_nodes[safe_idx];
        if !node.children.is_empty() {
            children_start_abi[safe_idx] = next_idx;
            for &child in &node.children {
                abi_index[child] = next_idx;
                next_idx += 1;
                queue.push_back(child);
            }
        }
    }

    // Fill ABI nodes
    for safe_idx in 0..n {
        let node = &safe_nodes[safe_idx];
        let abi_idx = abi_index[safe_idx];
        let abi_node = unsafe { &mut *dest.add(abi_idx) };

        match node.tag {
            Tag::Nil => {
                abi_node.type_ = TYPE_NIL;
                abi_node.len = 0;
                abi_node.value = 0;
            }
            Tag::Bool(v) => {
                abi_node.type_ = TYPE_BOOL;
                abi_node.len = 0;
                abi_node.value = v as u64;
            }
            Tag::Int(v) => {
                abi_node.type_ = TYPE_INT;
                abi_node.len = 0;
                abi_node.value = v as u64;
            }
            Tag::Uint(v) => {
                abi_node.type_ = TYPE_UINT;
                abi_node.len = 0;
                abi_node.value = v;
            }
            Tag::Float(v) => {
                abi_node.type_ = TYPE_FLOAT;
                abi_node.len = 0;
                abi_node.value = v.to_bits() as u64;
            }
            Tag::Double(v) => {
                abi_node.type_ = TYPE_DOUBLE;
                abi_node.len = 0;
                abi_node.value = v.to_bits();
            }
            Tag::Str(length) => {
                abi_node.type_ = TYPE_STR;
                abi_node.len = length;
                // value = byte offset into tree->data
                abi_node.value = node.payload_off as u64;
            }
            Tag::Bin(length) => {
                abi_node.type_ = TYPE_BIN;
                abi_node.len = length;
                abi_node.value = node.payload_off as u64;
            }
            Tag::Ext { length, .. } => {
                abi_node.type_ = TYPE_EXT;
                abi_node.len = length;
                // value = byte offset of ext payload; exttype is at offset-1 in tree->data
                abi_node.value = node.payload_off as u64;
            }
            Tag::Array(count) => {
                abi_node.type_ = TYPE_ARRAY;
                abi_node.len = count;
                if node.children.is_empty() {
                    abi_node.value = 0;
                } else {
                    let child_ptr = unsafe { dest.add(children_start_abi[safe_idx]) };
                    abi_node.value = child_ptr as u64;
                }
            }
            Tag::Map(count) => {
                abi_node.type_ = TYPE_MAP;
                abi_node.len = count;
                if node.children.is_empty() {
                    abi_node.value = 0;
                } else {
                    let child_ptr = unsafe { dest.add(children_start_abi[safe_idx]) };
                    abi_node.value = child_ptr as u64;
                }
            }
        }
    }
}

// ── core parse logic ──────────────────────────────────────────────────────────

/// Parse data and materialise nodes into pool or heap. Updates tree fields.
/// Returns true if parsing succeeded.
fn do_parse_inner(tree: *mut MpackTree, data: *const u8, data_length: usize) -> bool {
    if tree_error(tree) != MPACK_OK {
        return false;
    }
    if !has_state(tree) {
        clear_abi_parse_views(tree);
        flag_tree_error(tree, MPACK_ERROR_BUG);
        return false;
    }
    let t_ref = unsafe { &*tree };

    let (max_nodes, using_pool, pool_count) = with_state(
        tree,
        (usize::MAX, false, 0usize),
        |s| {
            let mn = if s.using_pool {
                t_ref.pool_count
            } else {
                s.max_nodes
            };
            (mn, s.using_pool, t_ref.pool_count)
        },
    );

    // `max_size` is a *message* / stream-accumulation limit (C `tree->max_size`),
    // not a cap on the whole `data_length` buffer. Multi-message `init_data`
    // buffers may exceed `max_size` while each message is fine. Stream fill
    // enforces the cap when growing `owned_data` (see `fill_stream`).

    // Parse via safe core
    let max_nodes_opt = if max_nodes == usize::MAX { None } else { Some(max_nodes) };

    let (nodes, root, error, size) = {
        let data_slice: &[u8] = if data.is_null() || data_length == 0 {
            &[][..]
        } else {
            unsafe { slice::from_raw_parts(data, data_length) }
        };
        Tree::parse_with_limits(data_slice, max_nodes_opt).into_parts()
    };

    let abi_error = core_error_to_abi(error);
    let node_count = nodes.len();

    if abi_error != MPACK_OK {
        clear_abi_parse_views(tree);
        flag_tree_error(tree, abi_error);
        return false;
    }

    if using_pool {
        // Write into user-provided pool
        if node_count > pool_count {
            clear_abi_parse_views(tree);
            flag_tree_error(tree, MPACK_ERROR_TOO_BIG);
            return false;
        }
        let pool = unsafe { (*tree).pool };
        if !pool.is_null() {
            // SAFETY: pool has pool_count elements; we wrote node_count <= pool_count
            unsafe { materialize_nodes(&nodes, root.unwrap_or(0), pool) };
            unsafe { (*tree).root = pool }; // root is at index 0 after BFS
        }
        with_state(tree, (), |s| {
            s.nodes = nodes;
            s.root = root;
            s.size = size;
            s.parsed = true;
        });
        let t = unsafe { &mut *tree };
        t.size = size;
        t.node_count = node_count;
        t.parser.state = 2; // mpack_tree_parse_state_parsed
    } else {
        // Write into heap-allocated Box
        let mut heap = vec![MpackNodeData::nil(); node_count.max(1)].into_boxed_slice();
        if node_count > 0 {
            // SAFETY: heap has node_count elements
            unsafe { materialize_nodes(&nodes, root.unwrap_or(0), heap.as_mut_ptr()) };
        }
        // Publish the side-table owner and the ABI root together. If state
        // disappears or its mutex is poisoned, `heap` drops locally while the
        // tree stays on its nil sentinel and fails closed below.
        let published = with_state(tree, false, |s| {
            s.nodes = nodes;
            s.root = root;
            s.size = size;
            s.parsed = true;
            s.heap_nodes = heap;
            let t = unsafe { &mut *tree };
            t.root = if node_count > 0 {
                s.heap_nodes.as_mut_ptr()
            } else {
                ptr::addr_of_mut!(t.nil_node)
            };
            t.size = size;
            t.node_count = node_count;
            t.parser.state = 2; // mpack_tree_parse_state_parsed
            true
        });
        if !published {
            clear_abi_parse_views(tree);
            flag_tree_error(tree, MPACK_ERROR_BUG);
            return false;
        }
    }

    true
}

/// Result of a streaming fill attempt.
struct FillOutcome {
    /// False if `read_fn` (or prior state) already flagged a tree error.
    ok: bool,
    /// True when `owned_data` reached `max_size` (C `reserve_fill` would refuse
    /// further growth with `mpack_error_too_big` if more bytes are still needed).
    hit_max_size: bool,
}

/// Streaming fill: read via `read_fn` into `owned_data`, capped by `max_size`
/// (C `tree->max_size` — max bytes accumulated for the current message).
fn fill_stream(tree: *mut MpackTree, blocking: bool) -> FillOutcome {
    let read_fn = unsafe { (*tree).read_fn };
    let Some(read_fn) = read_fn else {
        return FillOutcome {
            ok: true,
            hit_max_size: false,
        };
    };
    let max_size = with_state(tree, usize::MAX, |s| s.max_size);
    let mut chunk = vec![0u8; INITIAL_STREAM_CAPACITY];
    let mut hit_max_size = false;

    loop {
        let current_len = with_state(tree, 0usize, |s| s.owned_data.len());
        if current_len >= max_size {
            // Cap reached. If the message is still incomplete after parse,
            // `mpack_tree_parse` remaps Invalid → TOO_BIG (C `reserve_fill`).
            hit_max_size = true;
            break;
        }
        let room = max_size - current_len;
        let want = room.min(chunk.len());
        let read = unsafe { read_fn(tree, chunk.as_mut_ptr().cast(), want) };
        if tree_error(tree) != MPACK_OK {
            return FillOutcome {
                ok: false,
                hit_max_size,
            };
        }
        if read == 0 || read == usize::MAX {
            break;
        }
        let take = read.min(room);
        with_state(tree, (), |s| {
            s.owned_data.extend_from_slice(&chunk[..take]);
        });
        if current_len + take >= max_size {
            hit_max_size = true;
            break;
        }
        if !blocking {
            // For try_parse: only one read round
            break;
        }
    }

    // Point tree.data at owned_data (stable heap pointer)
    with_state(tree, (), |s| {
        let t = unsafe { &mut *tree };
        t.data = s.owned_data.as_ptr().cast();
        t.data_length = s.owned_data.len();
    });
    FillOutcome {
        ok: true,
        hit_max_size,
    }
}

// ── default init helper ───────────────────────────────────────────────────────

fn init_tree_clear(tree: *mut MpackTree) {
    if tree.is_null() {
        return;
    }
    let t = unsafe { &mut *tree };
    t.error_fn = None;
    t.read_fn = None;
    t.teardown = None;
    t.context = ptr::null_mut();
    t.nil_node = MpackNodeData::nil();
    t.missing_node = MpackNodeData::missing();
    t.error = MPACK_OK;
    t.buffer = ptr::null_mut();
    t.buffer_capacity = 0;
    t.data = ptr::null();
    t.data_length = 0;
    t.size = 0;
    t.node_count = 0;
    t.max_size = usize::MAX;
    t.max_nodes = usize::MAX;
    t.parser = MpackTreeParser::empty();
    // Root points at embedded nil sentinel until a successful parse
    t.root = ptr::addr_of_mut!(t.nil_node);
    t.pool = ptr::null_mut();
    t.pool_count = 0;
    t.next = ptr::null_mut();

    register_state(
        tree,
        FfiTreeState {
            nodes: Vec::new(),
            root: None,
            size: 0,
            parsed: false,
            heap_nodes: Box::new([]),
            using_pool: false,
            owned_data: Vec::new(),
            close_file: None,
            max_size: usize::MAX,
            max_nodes: usize::MAX,
        },
    );
}

// ── tree init / destroy ───────────────────────────────────────────────────────

/// Initialises a tree over caller-owned data (no copy, no file I/O).
///
/// # Safety
/// `tree` must be non-null writable storage for one `mpack_tree_t`.
/// When `length > 0`, `data` must be readable for `length` bytes.
#[no_mangle]
pub unsafe extern "C" fn mpack_tree_init_data(
    tree: *mut MpackTree,
    data: *const c_char,
    length: usize,
) {
    if tree.is_null() {
        return;
    }
    if catch_ffi_panic(|| {
        init_tree_clear(tree);
        let t = unsafe { &mut *tree };
        t.data = data;
        t.data_length = length;
    })
    .is_err()
    {
        flag_tree_error(tree, MPACK_ERROR_BUG);
    }
}

/// Initialises a streaming tree.
///
/// # Safety
/// `tree` must be non-null writable storage for one `mpack_tree_t`.
#[no_mangle]
pub unsafe extern "C" fn mpack_tree_init_stream(
    tree: *mut MpackTree,
    read_fn: MpackTreeRead,
    context: *mut c_void,
    max_size: usize,
    max_nodes: usize,
) {
    if tree.is_null() {
        return;
    }
    if catch_ffi_panic(|| {
        init_tree_clear(tree);
        let t = unsafe { &mut *tree };
        t.read_fn = read_fn;
        t.context = context;
        t.max_size = max_size;
        t.max_nodes = max_nodes;
        with_state(tree, (), |s| {
            s.max_size = max_size;
            s.max_nodes = max_nodes;
            s.owned_data = Vec::with_capacity(INITIAL_STREAM_CAPACITY);
        });
    })
    .is_err()
    {
        flag_tree_error(tree, MPACK_ERROR_BUG);
    }
}

/// Initialises a tree with a user-provided node pool (no allocations).
///
/// # Safety
/// `tree` must be non-null writable for `mpack_tree_t`.
/// When `pool_count > 0`, `pool` must be writable for `pool_count * sizeof(mpack_node_data_t)`.
#[no_mangle]
pub unsafe extern "C" fn mpack_tree_init_pool(
    tree: *mut MpackTree,
    data: *const c_char,
    length: usize,
    pool: *mut MpackNodeData,
    pool_count: usize,
) {
    if tree.is_null() {
        return;
    }
    if catch_ffi_panic(|| {
        if pool_count == 0 {
            break_hit(b"pool_count must be > 0\0");
            init_tree_clear(tree);
            flag_tree_error(tree, MPACK_ERROR_BUG);
            return;
        }
        init_tree_clear(tree);
        let t = unsafe { &mut *tree };
        t.data = data;
        t.data_length = length;
        t.pool = pool;
        t.pool_count = pool_count;
        t.max_nodes = pool_count;
        with_state(tree, (), |s| {
            s.using_pool = true;
            s.max_nodes = pool_count;
        });
    })
    .is_err()
    {
        flag_tree_error(tree, MPACK_ERROR_BUG);
    }
}

/// Initialises a tree from a file loaded entirely into memory.
///
/// # Safety
/// `tree` must be non-null writable for `mpack_tree_t`.
/// `filename` must be a valid NUL-terminated C string.
#[no_mangle]
pub unsafe extern "C" fn mpack_tree_init_filename(
    tree: *mut MpackTree,
    filename: *const c_char,
    max_bytes: usize,
) {
    if tree.is_null() {
        return;
    }
    if catch_ffi_panic(|| {
        if filename.is_null() {
            init_tree_clear(tree);
            flag_tree_error(tree, MPACK_ERROR_BUG);
            return;
        }
        // SAFETY: filename is a NUL-terminated C string per the C contract
        let file = unsafe { fopen(filename, c"rb".as_ptr()) };
        if file.is_null() {
            init_tree_clear(tree);
            flag_tree_error(tree, MPACK_ERROR_IO);
            return;
        }
        init_tree_clear(tree);
        match load_file_data(file, max_bytes) {
            Ok(data) => {
                let stored = with_state(tree, false, |s| {
                    s.owned_data = data;
                    let t = unsafe { &mut *tree };
                    t.data = s.owned_data.as_ptr().cast();
                    t.data_length = s.owned_data.len();
                    true
                });
                if !stored {
                    clear_abi_parse_views(tree);
                    flag_tree_error(tree, MPACK_ERROR_BUG);
                }
            }
            Err(err) => {
                flag_tree_error(tree, err);
            }
        }
        // SAFETY: file was opened with fopen; we close after reading
        unsafe { fclose(file) };
    })
    .is_err()
    {
        flag_tree_error(tree, MPACK_ERROR_BUG);
    }
}

/// Initialises a tree from an existing `FILE*`.
///
/// # Safety
/// `tree` must be non-null writable for `mpack_tree_t`.
/// `stdfile` must be a live `FILE*`.
#[no_mangle]
pub unsafe extern "C" fn mpack_tree_init_stdfile(
    tree: *mut MpackTree,
    stdfile: *mut c_void,
    max_bytes: usize,
    close_when_done: bool,
) {
    if tree.is_null() {
        return;
    }
    if catch_ffi_panic(|| {
        if stdfile.is_null() {
            init_tree_clear(tree);
            flag_tree_error(tree, MPACK_ERROR_BUG);
            return;
        }
        init_tree_clear(tree);
        match load_file_data(stdfile, max_bytes) {
            Ok(data) => {
                let stored = with_state(tree, false, |s| {
                    s.owned_data = data;
                    if close_when_done {
                        s.close_file = Some(stdfile);
                    }
                    let t = unsafe { &mut *tree };
                    t.data = s.owned_data.as_ptr().cast();
                    t.data_length = s.owned_data.len();
                    true
                });
                if !stored {
                    clear_abi_parse_views(tree);
                    if close_when_done {
                        unsafe { fclose(stdfile) };
                    }
                    flag_tree_error(tree, MPACK_ERROR_BUG);
                }
            }
            Err(err) => {
                if close_when_done {
                    unsafe { fclose(stdfile) };
                }
                flag_tree_error(tree, err);
            }
        }
    })
    .is_err()
    {
        flag_tree_error(tree, MPACK_ERROR_BUG);
    }
}

/// Initialises a tree directly in an error state.
///
/// # Safety
/// `tree` must be non-null writable for `mpack_tree_t`.
#[no_mangle]
pub unsafe extern "C" fn mpack_tree_init_error(tree: *mut MpackTree, error: MpackError) {
    if tree.is_null() {
        return;
    }
    if catch_ffi_panic(|| {
        init_tree_clear(tree);
        flag_tree_error(tree, error);
    })
    .is_err()
    {
        flag_tree_error(tree, MPACK_ERROR_BUG);
    }
}

/// Sets the maximum message size and node count for a tree.
///
/// # Safety
/// `tree` must be non-null and live.
#[no_mangle]
pub unsafe extern "C" fn mpack_tree_set_limits(
    tree: *mut MpackTree,
    max_message_size: usize,
    max_message_nodes: usize,
) {
    if tree.is_null() {
        return;
    }
    if catch_ffi_panic(|| {
        if max_message_size == 0 || max_message_nodes == 0 {
            break_hit(b"set_limits: sizes must be > 0\0");
            flag_tree_error(tree, MPACK_ERROR_BUG);
            return;
        }
        let t = unsafe { &mut *tree };
        t.max_size = max_message_size;
        t.max_nodes = max_message_nodes;
        with_state(tree, (), |s| {
            s.max_size = max_message_size;
            s.max_nodes = max_message_nodes;
        });
    })
    .is_err()
    {
        flag_tree_error(tree, MPACK_ERROR_BUG);
    }
}

fn load_file_data(file: *mut c_void, max_bytes: usize) -> Result<Vec<u8>, MpackError> {
    // Align with C `mpack_file_tree_read` (mpack-node.c).
    if unsafe { fseek(file, 0, SEEK_END) } != 0 {
        return Err(MPACK_ERROR_IO);
    }
    let file_size_raw = unsafe { ftell(file) };
    if unsafe { fseek(file, 0, SEEK_SET) } != 0 {
        return Err(MPACK_ERROR_IO);
    }
    let file_size = match crate::node::check_file_tree_bytes(file_size_raw, max_bytes) {
        Ok(size) => size,
        Err(crate::common::Error::Invalid) => return Err(MPACK_ERROR_INVALID),
        Err(crate::common::Error::TooBig) => return Err(MPACK_ERROR_TOO_BIG),
        Err(_) => return Err(MPACK_ERROR_IO),
    };
    let mut buf = vec![0u8; file_size];
    let read = unsafe { fread(buf.as_mut_ptr().cast(), 1, file_size, file) };
    if read != file_size {
        return Err(MPACK_ERROR_IO);
    }
    Ok(buf)
}

// ── parse ─────────────────────────────────────────────────────────────────────

/// Parses one MessagePack message from the tree's data source (blocking).
///
/// # Safety
/// `tree` must be non-null and live.
#[no_mangle]
pub unsafe extern "C" fn mpack_tree_parse(tree: *mut MpackTree) {
    if tree.is_null() {
        return;
    }
    let _ = catch_ffi_panic(|| {
        if tree_error(tree) != MPACK_OK {
            return;
        }
        advance_previous_message(tree);
        let has_read_fn = unsafe { (*tree).read_fn }.is_some();
        let mut hit_max_size = false;
        if has_read_fn {
            let fill = fill_stream(tree, true);
            if !fill.ok {
                return;
            }
            hit_max_size = fill.hit_max_size;
        }
        let (data, data_length) = {
            let t = unsafe { &*tree };
            (t.data.cast::<u8>(), t.data_length)
        };
        if !do_parse_inner(tree, data, data_length) && has_read_fn {
            // Safe-core flags Invalid/Eof for truncated buffers. With a read_fn,
            // C's blocking parse maps incomplete → IO, or TOO_BIG when the fill
            // already hit max_size (reserve_fill). Remap after the fact.
            remap_stream_incomplete(tree, hit_max_size);
        }
    });
}

/// Non-blocking parse attempt. Returns false if more data is needed.
///
/// # Safety
/// `tree` must be non-null and live.
#[no_mangle]
pub unsafe extern "C" fn mpack_tree_try_parse(tree: *mut MpackTree) -> bool {
    if tree.is_null() {
        return false;
    }
    match catch_ffi_panic(|| {
        if tree_error(tree) != MPACK_OK {
            return false;
        }
        advance_previous_message(tree);
        let has_read_fn = unsafe { (*tree).read_fn }.is_some();
        let mut hit_max_size = false;
        if has_read_fn {
            // Non-blocking: read one round of available data
            let fill = fill_stream(tree, false);
            if !fill.ok {
                return false;
            }
            hit_max_size = fill.hit_max_size;
        }
        let (data, data_length) = {
            let t = unsafe { &*tree };
            (t.data.cast::<u8>(), t.data_length)
        };

        // Attempt parse; Invalid/Eof may mean incomplete data (need another fill).
        let prev_error = tree_error(tree);
        let ok = do_parse_inner(tree, data, data_length);
        if !ok {
            let err = tree_error(tree);
            if err == MPACK_ERROR_INVALID || err == MPACK_ERROR_EOF {
                if has_read_fn && hit_max_size {
                    // Cap hit and still incomplete → TOO_BIG (C reserve_fill).
                    replace_tree_error(tree, MPACK_ERROR_TOO_BIG);
                    return false;
                }
                // Incomplete: reset error so caller can retry
                clear_abi_parse_views(tree);
                unsafe { (*tree).error = prev_error };
                return false;
            }
        }
        ok
    }) {
        Ok(result) => result,
        Err(_) => {
            flag_tree_error(tree, MPACK_ERROR_BUG);
            false
        }
    }
}

/// Returns the root node of a successfully parsed tree.
///
/// # Safety
/// `tree` must be non-null and live.
#[no_mangle]
pub unsafe extern "C" fn mpack_tree_root(tree: *mut MpackTree) -> MpackNode {
    if tree.is_null() {
        return MpackNode::null();
    }
    match catch_ffi_panic(|| {
        if tree_error(tree) != MPACK_OK {
            return nil_node_for(tree);
        }
        let parsed = with_state(tree, false, |s| s.parsed);
        if !parsed {
            break_hit(b"mpack_tree_root called before mpack_tree_parse\0");
            flag_tree_error(tree, MPACK_ERROR_BUG);
            return nil_node_for(tree);
        }
        let t = unsafe { &*tree };
        MpackNode {
            data: t.root,
            tree,
        }
    }) {
        Ok(node) => node,
        Err(_) => {
            flag_tree_error(tree, MPACK_ERROR_BUG);
            nil_node_for(tree)
        }
    }
}

/// Flags a sticky error on a tree.
///
/// # Safety
/// `tree` must be non-null and live.
#[no_mangle]
pub unsafe extern "C" fn mpack_tree_flag_error(tree: *mut MpackTree, error: MpackError) {
    if tree.is_null() {
        return;
    }
    let _ = catch_ffi_panic(|| {
        flag_tree_error(tree, error);
    });
}

/// Destroys a tree, calling its teardown and releasing owned resources.
///
/// # Safety
/// `tree` must be non-null and live.
#[no_mangle]
pub unsafe extern "C" fn mpack_tree_destroy(tree: *mut MpackTree) -> MpackError {
    if tree.is_null() {
        return MPACK_ERROR_BUG;
    }
    match catch_ffi_panic(|| {
        // Revoke ABI views before the side-table drops their heap owners.
        clear_abi_parse_views(tree);
        let state = remove_state(tree);
        if let Some(s) = state {
            if let Some(file) = s.close_file {
                unsafe { fclose(file) };
            }
        }

        // C cleans parser pages/buffers before calling teardown.
        let teardown = unsafe { (*tree).teardown.take() };
        if let Some(td) = teardown {
            unsafe { td(tree) };
        }

        tree_error(tree)
    }) {
        Ok(error) => error,
        Err(_) => MPACK_ERROR_BUG,
    }
}

// ── node tag / type ───────────────────────────────────────────────────────────

/// Returns the tag for a node.
#[no_mangle]
pub unsafe extern "C" fn mpack_node_tag(node: MpackNode) -> MpackTag {
    if node.data.is_null() || node.tree.is_null() {
        return MpackTag::nil();
    }
    match catch_ffi_panic(|| {
        if node_tree_error(node) != MPACK_OK {
            return MpackTag::nil();
        }
        let nd = unsafe { &*node.data };
        abi_node_to_tag(nd, tree_data_ptr(node))
    }) {
        Ok(tag) => tag,
        Err(_) => {
            flag_tree_error(node.tree, MPACK_ERROR_BUG);
            MpackTag::nil()
        }
    }
}

fn abi_node_to_tag(nd: &MpackNodeData, tree_data: *const u8) -> MpackTag {
    let zero_tag = |t: c_int, v: u64| MpackTag {
        type_: t,
        exttype: 0,
        _pad: [0; 3],
        value: v,
    };
    match nd.type_ {
        TYPE_MISSING => MpackTag::zero(),
        TYPE_NIL => MpackTag::nil(),
        TYPE_BOOL => zero_tag(TYPE_BOOL, nd.value & 0xff),
        TYPE_INT => zero_tag(TYPE_INT, nd.value),
        TYPE_UINT => zero_tag(TYPE_UINT, nd.value),
        TYPE_FLOAT => zero_tag(TYPE_FLOAT, nd.value & 0xffff_ffff),
        TYPE_DOUBLE => zero_tag(TYPE_DOUBLE, nd.value),
        TYPE_STR => zero_tag(TYPE_STR, nd.len as u64),
        TYPE_BIN => zero_tag(TYPE_BIN, nd.len as u64),
        TYPE_ARRAY => zero_tag(TYPE_ARRAY, nd.len as u64),
        TYPE_MAP => zero_tag(TYPE_MAP, nd.len as u64),
        TYPE_EXT => {
            let exttype = if !tree_data.is_null() && nd.value > 0 {
                // exttype is stored one byte before the ext payload
                unsafe { *tree_data.add(nd.value as usize - 1) as i8 }
            } else {
                0i8
            };
            MpackTag {
                type_: TYPE_EXT,
                exttype,
                _pad: [0; 3],
                value: nd.len as u64,
            }
        }
        _ => MpackTag::nil(),
    }
}

/// Returns the type of a node as an integer constant.
#[no_mangle]
pub unsafe extern "C" fn mpack_node_type(node: MpackNode) -> c_int {
    if node.tree.is_null() || node.data.is_null() {
        return TYPE_NIL;
    }
    match catch_ffi_panic(|| {
        if node_tree_error(node) != MPACK_OK {
            return TYPE_NIL;
        }
        nd_type(node)
    }) {
        Ok(t) => t,
        Err(_) => {
            flag_tree_error(node.tree, MPACK_ERROR_BUG);
            TYPE_NIL
        }
    }
}

/// Returns whether the node's type is nil (or the tree has an error).
#[no_mangle]
pub unsafe extern "C" fn mpack_node_is_nil(node: MpackNode) -> bool {
    if node.tree.is_null() || node.data.is_null() {
        return true;
    }
    // Per C MPack: is_nil is true if tree has error OR type is nil
    if node_tree_error(node) != MPACK_OK {
        return true;
    }
    nd_type(node) == TYPE_NIL
}

/// Returns whether the node represents a missing optional value.
#[no_mangle]
pub unsafe extern "C" fn mpack_node_is_missing(node: MpackNode) -> bool {
    if node.tree.is_null() || node.data.is_null() {
        return false;
    }
    // Per C MPack: is_missing is false if tree has error
    if node_tree_error(node) != MPACK_OK {
        return false;
    }
    nd_type(node) == TYPE_MISSING
}

// ── nil / true / false / bool ─────────────────────────────────────────────────

/// Expects a nil node; flags type error otherwise.
#[no_mangle]
pub unsafe extern "C" fn mpack_node_nil(node: MpackNode) {
    if node.tree.is_null() {
        return;
    }
    let _ = catch_ffi_panic(|| {
        if node_tree_error(node) != MPACK_OK {
            return;
        }
        if nd_type(node) != TYPE_NIL {
            flag_tree_error(node.tree, MPACK_ERROR_TYPE);
        }
    });
}

/// Expects a true node; flags type error otherwise.
#[no_mangle]
pub unsafe extern "C" fn mpack_node_true(node: MpackNode) {
    if node.tree.is_null() {
        return;
    }
    let _ = catch_ffi_panic(|| {
        if node_tree_error(node) != MPACK_OK {
            return;
        }
        if nd_type(node) != TYPE_BOOL || nd_value(node) == 0 {
            flag_tree_error(node.tree, MPACK_ERROR_TYPE);
        }
    });
}

/// Expects a false node; flags type error otherwise.
#[no_mangle]
pub unsafe extern "C" fn mpack_node_false(node: MpackNode) {
    if node.tree.is_null() {
        return;
    }
    let _ = catch_ffi_panic(|| {
        if node_tree_error(node) != MPACK_OK {
            return;
        }
        if nd_type(node) != TYPE_BOOL || nd_value(node) != 0 {
            flag_tree_error(node.tree, MPACK_ERROR_TYPE);
        }
    });
}

/// Returns the bool value; flags type error on mismatch.
#[no_mangle]
pub unsafe extern "C" fn mpack_node_bool(node: MpackNode) -> bool {
    if node.tree.is_null() {
        return false;
    }
    match catch_ffi_panic(|| {
        if node_tree_error(node) != MPACK_OK {
            return false;
        }
        if nd_type(node) != TYPE_BOOL {
            flag_tree_error(node.tree, MPACK_ERROR_TYPE);
            return false;
        }
        nd_value(node) != 0
    }) {
        Ok(v) => v,
        Err(_) => {
            flag_tree_error(node.tree, MPACK_ERROR_BUG);
            false
        }
    }
}

// ── scalar integers ───────────────────────────────────────────────────────────

/// Reads uint/non-negative-int as u64; type error on mismatch.
fn node_as_u64_impl(node: MpackNode) -> u64 {
    if node_tree_error(node) != MPACK_OK {
        return 0;
    }
    match nd_type(node) {
        TYPE_UINT => nd_value(node),
        TYPE_INT => {
            let v = nd_value(node) as i64;
            if v >= 0 {
                v as u64
            } else {
                flag_tree_error(node.tree, MPACK_ERROR_TYPE);
                0
            }
        }
        _ => {
            flag_tree_error(node.tree, MPACK_ERROR_TYPE);
            0
        }
    }
}

/// Reads int/non-negative-uint as i64; type error on mismatch.
fn node_as_i64_impl(node: MpackNode) -> i64 {
    if node_tree_error(node) != MPACK_OK {
        return 0;
    }
    match nd_type(node) {
        TYPE_INT => nd_value(node) as i64,
        TYPE_UINT => {
            let v = nd_value(node);
            if v <= i64::MAX as u64 {
                v as i64
            } else {
                flag_tree_error(node.tree, MPACK_ERROR_TYPE);
                0
            }
        }
        _ => {
            flag_tree_error(node.tree, MPACK_ERROR_TYPE);
            0
        }
    }
}

macro_rules! node_uint_fn {
    ($name:ident, $ty:ty) => {
        #[no_mangle]
        pub unsafe extern "C" fn $name(node: MpackNode) -> $ty {
            if node.tree.is_null() {
                return 0;
            }
            match catch_ffi_panic(|| {
                let v = node_as_u64_impl(node);
                if node_tree_error(node) != MPACK_OK {
                    return 0 as $ty;
                }
                if v > <$ty>::MAX as u64 {
                    flag_tree_error(node.tree, MPACK_ERROR_TYPE);
                    return 0 as $ty;
                }
                v as $ty
            }) {
                Ok(v) => v,
                Err(_) => {
                    flag_tree_error(node.tree, MPACK_ERROR_BUG);
                    0 as $ty
                }
            }
        }
    };
}

macro_rules! node_sint_fn {
    ($name:ident, $ty:ty) => {
        #[no_mangle]
        pub unsafe extern "C" fn $name(node: MpackNode) -> $ty {
            if node.tree.is_null() {
                return 0;
            }
            match catch_ffi_panic(|| {
                let v = node_as_i64_impl(node);
                if node_tree_error(node) != MPACK_OK {
                    return 0 as $ty;
                }
                if v < <$ty>::MIN as i64 || v > <$ty>::MAX as i64 {
                    flag_tree_error(node.tree, MPACK_ERROR_TYPE);
                    return 0 as $ty;
                }
                v as $ty
            }) {
                Ok(v) => v,
                Err(_) => {
                    flag_tree_error(node.tree, MPACK_ERROR_BUG);
                    0 as $ty
                }
            }
        }
    };
}

node_uint_fn!(mpack_node_u8, u8);
node_uint_fn!(mpack_node_u16, u16);
node_uint_fn!(mpack_node_u32, u32);
node_sint_fn!(mpack_node_i8, i8);
node_sint_fn!(mpack_node_i16, i16);
node_sint_fn!(mpack_node_i32, i32);

#[no_mangle]
pub unsafe extern "C" fn mpack_node_u64(node: MpackNode) -> u64 {
    if node.tree.is_null() {
        return 0;
    }
    match catch_ffi_panic(|| node_as_u64_impl(node)) {
        Ok(v) => v,
        Err(_) => {
            flag_tree_error(node.tree, MPACK_ERROR_BUG);
            0
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_i64(node: MpackNode) -> i64 {
    if node.tree.is_null() {
        return 0;
    }
    match catch_ffi_panic(|| node_as_i64_impl(node)) {
        Ok(v) => v,
        Err(_) => {
            flag_tree_error(node.tree, MPACK_ERROR_BUG);
            0
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_int(node: MpackNode) -> c_int {
    if node.tree.is_null() {
        return 0;
    }
    match catch_ffi_panic(|| {
        let v = node_as_i64_impl(node);
        if node_tree_error(node) != MPACK_OK {
            return 0;
        }
        if v < c_int::MIN as i64 || v > c_int::MAX as i64 {
            flag_tree_error(node.tree, MPACK_ERROR_TYPE);
            return 0;
        }
        v as c_int
    }) {
        Ok(v) => v,
        Err(_) => {
            flag_tree_error(node.tree, MPACK_ERROR_BUG);
            0
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_uint(node: MpackNode) -> c_uint {
    if node.tree.is_null() {
        return 0;
    }
    match catch_ffi_panic(|| {
        let v = node_as_u64_impl(node);
        if node_tree_error(node) != MPACK_OK {
            return 0;
        }
        if v > c_uint::MAX as u64 {
            flag_tree_error(node.tree, MPACK_ERROR_TYPE);
            return 0;
        }
        v as c_uint
    }) {
        Ok(v) => v,
        Err(_) => {
            flag_tree_error(node.tree, MPACK_ERROR_BUG);
            0
        }
    }
}

// ── scalar floats ─────────────────────────────────────────────────────────────

/// Returns float; accepts float, double, uint, int (widening). Strict variant:
/// float only.
fn node_float_impl(node: MpackNode, strict: bool) -> f32 {
    if node_tree_error(node) != MPACK_OK {
        return 0.0;
    }
    match nd_type(node) {
        TYPE_FLOAT => f32::from_bits(nd_value(node) as u32),
        TYPE_DOUBLE if !strict => f64::from_bits(nd_value(node)) as f32,
        TYPE_INT if !strict => nd_value(node) as i64 as f32,
        TYPE_UINT if !strict => nd_value(node) as f32,
        _ => {
            flag_tree_error(node.tree, MPACK_ERROR_TYPE);
            0.0
        }
    }
}

fn node_double_impl(node: MpackNode, strict: bool) -> f64 {
    if node_tree_error(node) != MPACK_OK {
        return 0.0;
    }
    // C `double_strict` still widens float→double; ints only in non-strict.
    match nd_type(node) {
        TYPE_DOUBLE => f64::from_bits(nd_value(node)),
        TYPE_FLOAT => f32::from_bits(nd_value(node) as u32) as f64,
        TYPE_INT if !strict => nd_value(node) as i64 as f64,
        TYPE_UINT if !strict => nd_value(node) as f64,
        _ => {
            flag_tree_error(node.tree, MPACK_ERROR_TYPE);
            0.0
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_float(node: MpackNode) -> f32 {
    if node.tree.is_null() {
        return 0.0;
    }
    match catch_ffi_panic(|| node_float_impl(node, false)) {
        Ok(v) => v,
        Err(_) => {
            flag_tree_error(node.tree, MPACK_ERROR_BUG);
            0.0
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_double(node: MpackNode) -> f64 {
    if node.tree.is_null() {
        return 0.0;
    }
    match catch_ffi_panic(|| node_double_impl(node, false)) {
        Ok(v) => v,
        Err(_) => {
            flag_tree_error(node.tree, MPACK_ERROR_BUG);
            0.0
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_float_strict(node: MpackNode) -> f32 {
    if node.tree.is_null() {
        return 0.0;
    }
    match catch_ffi_panic(|| node_float_impl(node, true)) {
        Ok(v) => v,
        Err(_) => {
            flag_tree_error(node.tree, MPACK_ERROR_BUG);
            0.0
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_double_strict(node: MpackNode) -> f64 {
    if node.tree.is_null() {
        return 0.0;
    }
    match catch_ffi_panic(|| node_double_impl(node, true)) {
        Ok(v) => v,
        Err(_) => {
            flag_tree_error(node.tree, MPACK_ERROR_BUG);
            0.0
        }
    }
}

// ── timestamp ─────────────────────────────────────────────────────────────────

fn decode_ext_timestamp(tree_data: *const u8, offset: usize, len: u32) -> Option<MpackTimestamp> {
    if tree_data.is_null() {
        return None;
    }
    let bytes = unsafe { slice::from_raw_parts(tree_data.add(offset), len as usize) };
    match len {
        4 => {
            let secs = u32::from_be_bytes(bytes.try_into().ok()?) as i64;
            Some(MpackTimestamp {
                seconds: secs,
                nanoseconds: 0,
            })
        }
        8 => {
            let packed = u64::from_be_bytes(bytes.try_into().ok()?);
            let ns = (packed >> 34) as u32;
            let secs = (packed & ((1u64 << 34) - 1)) as i64;
            if ns > 999_999_999 {
                return None;
            }
            Some(MpackTimestamp {
                seconds: secs,
                nanoseconds: ns,
            })
        }
        12 => {
            let ns = u32::from_be_bytes(bytes[0..4].try_into().ok()?);
            let secs = i64::from_be_bytes(bytes[4..12].try_into().ok()?);
            if ns > 999_999_999 {
                return None;
            }
            Some(MpackTimestamp {
                seconds: secs,
                nanoseconds: ns,
            })
        }
        _ => None,
    }
}

const MPACK_EXTTYPE_TIMESTAMP: i8 = -1;

#[no_mangle]
pub unsafe extern "C" fn mpack_node_timestamp(node: MpackNode) -> MpackTimestamp {
    let zero = MpackTimestamp {
        seconds: 0,
        nanoseconds: 0,
    };
    if node.tree.is_null() {
        return zero;
    }
    match catch_ffi_panic(|| {
        if node_tree_error(node) != MPACK_OK {
            return zero;
        }
        let nd = unsafe { &*node.data };
        if nd.type_ != TYPE_EXT {
            flag_tree_error(node.tree, MPACK_ERROR_TYPE);
            return zero;
        }
        let tree_data = tree_data_ptr(node);
        let offset = nd.value as usize;
        // exttype is at offset - 1
        let exttype = if !tree_data.is_null() && offset > 0 {
            unsafe { *tree_data.add(offset - 1) as i8 }
        } else {
            0
        };
        if exttype != MPACK_EXTTYPE_TIMESTAMP {
            flag_tree_error(node.tree, MPACK_ERROR_TYPE);
            return zero;
        }
        match decode_ext_timestamp(tree_data, offset, nd.len) {
            Some(ts) => ts,
            None => {
                flag_tree_error(node.tree, MPACK_ERROR_INVALID);
                zero
            }
        }
    }) {
        Ok(ts) => ts,
        Err(_) => {
            flag_tree_error(node.tree, MPACK_ERROR_BUG);
            zero
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_timestamp_seconds(node: MpackNode) -> i64 {
    unsafe { mpack_node_timestamp(node) }.seconds
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_timestamp_nanoseconds(node: MpackNode) -> u32 {
    unsafe { mpack_node_timestamp(node) }.nanoseconds
}

// ── ext type ──────────────────────────────────────────────────────────────────

#[no_mangle]
pub unsafe extern "C" fn mpack_node_exttype(node: MpackNode) -> i8 {
    if node.tree.is_null() {
        return 0;
    }
    match catch_ffi_panic(|| {
        if node_tree_error(node) != MPACK_OK {
            return 0i8;
        }
        let nd = unsafe { &*node.data };
        if nd.type_ != TYPE_EXT {
            flag_tree_error(node.tree, MPACK_ERROR_TYPE);
            return 0;
        }
        let tree_data = tree_data_ptr(node);
        let offset = nd.value as usize;
        if tree_data.is_null() || offset == 0 {
            return 0;
        }
        unsafe { *tree_data.add(offset - 1) as i8 }
    }) {
        Ok(v) => v,
        Err(_) => {
            flag_tree_error(node.tree, MPACK_ERROR_BUG);
            0
        }
    }
}

// ── data length / pointers ────────────────────────────────────────────────────

/// Returns byte length for str, bin, or ext nodes.
#[no_mangle]
pub unsafe extern "C" fn mpack_node_data_len(node: MpackNode) -> u32 {
    if node.tree.is_null() {
        return 0;
    }
    match catch_ffi_panic(|| {
        if node_tree_error(node) != MPACK_OK {
            return 0u32;
        }
        match nd_type(node) {
            TYPE_STR | TYPE_BIN | TYPE_EXT => nd_len(node),
            _ => {
                flag_tree_error(node.tree, MPACK_ERROR_TYPE);
                0
            }
        }
    }) {
        Ok(v) => v,
        Err(_) => {
            flag_tree_error(node.tree, MPACK_ERROR_BUG);
            0
        }
    }
}

/// Returns byte length for str nodes only.
#[no_mangle]
pub unsafe extern "C" fn mpack_node_strlen(node: MpackNode) -> usize {
    if node.tree.is_null() {
        return 0;
    }
    match catch_ffi_panic(|| {
        if node_tree_error(node) != MPACK_OK {
            return 0usize;
        }
        if nd_type(node) != TYPE_STR {
            flag_tree_error(node.tree, MPACK_ERROR_TYPE);
            return 0;
        }
        nd_len(node) as usize
    }) {
        Ok(v) => v,
        Err(_) => {
            flag_tree_error(node.tree, MPACK_ERROR_BUG);
            0
        }
    }
}

/// Returns byte size of a bin node.
#[no_mangle]
pub unsafe extern "C" fn mpack_node_bin_size(node: MpackNode) -> usize {
    if node.tree.is_null() {
        return 0;
    }
    match catch_ffi_panic(|| {
        if node_tree_error(node) != MPACK_OK {
            return 0usize;
        }
        if nd_type(node) != TYPE_BIN {
            flag_tree_error(node.tree, MPACK_ERROR_TYPE);
            return 0;
        }
        nd_len(node) as usize
    }) {
        Ok(v) => v,
        Err(_) => {
            flag_tree_error(node.tree, MPACK_ERROR_BUG);
            0
        }
    }
}

fn node_data_pointer_impl(node: MpackNode, expected_type: c_int) -> *const c_char {
    if node_tree_error(node) != MPACK_OK {
        return ptr::null();
    }
    if nd_type(node) != expected_type {
        flag_tree_error(node.tree, MPACK_ERROR_TYPE);
        return ptr::null();
    }
    let tree_data = tree_data_ptr(node);
    if tree_data.is_null() {
        return ptr::null();
    }
    let offset = nd_value(node) as usize;
    unsafe { tree_data.add(offset) as *const c_char }
}

fn node_data_pointer_multi_impl(node: MpackNode, accept_ext: bool) -> *const c_char {
    if node_tree_error(node) != MPACK_OK {
        return ptr::null();
    }
    let t = nd_type(node);
    let ok = t == TYPE_STR
        || t == TYPE_BIN
        || (accept_ext && t == TYPE_EXT);
    if !ok {
        flag_tree_error(node.tree, MPACK_ERROR_TYPE);
        return ptr::null();
    }
    let tree_data = tree_data_ptr(node);
    if tree_data.is_null() {
        return ptr::null();
    }
    let offset = nd_value(node) as usize;
    unsafe { tree_data.add(offset) as *const c_char }
}

/// Returns pointer to str data.
#[no_mangle]
pub unsafe extern "C" fn mpack_node_str(node: MpackNode) -> *const c_char {
    if node.tree.is_null() {
        return ptr::null();
    }
    match catch_ffi_panic(|| node_data_pointer_impl(node, TYPE_STR)) {
        Ok(p) => p,
        Err(_) => {
            flag_tree_error(node.tree, MPACK_ERROR_BUG);
            ptr::null()
        }
    }
}

/// Returns pointer to data for str, bin, or ext.
#[no_mangle]
pub unsafe extern "C" fn mpack_node_data(node: MpackNode) -> *const c_char {
    if node.tree.is_null() {
        return ptr::null();
    }
    match catch_ffi_panic(|| node_data_pointer_multi_impl(node, true)) {
        Ok(p) => p,
        Err(_) => {
            flag_tree_error(node.tree, MPACK_ERROR_BUG);
            ptr::null()
        }
    }
}

/// Returns pointer to bin data.
#[no_mangle]
pub unsafe extern "C" fn mpack_node_bin_data(node: MpackNode) -> *const c_char {
    if node.tree.is_null() {
        return ptr::null();
    }
    match catch_ffi_panic(|| node_data_pointer_impl(node, TYPE_BIN)) {
        Ok(p) => p,
        Err(_) => {
            flag_tree_error(node.tree, MPACK_ERROR_BUG);
            ptr::null()
        }
    }
}

// ── UTF-8 checks ──────────────────────────────────────────────────────────────

#[no_mangle]
pub unsafe extern "C" fn mpack_node_check_utf8(node: MpackNode) {
    if node.tree.is_null() {
        return;
    }
    let _ = catch_ffi_panic(|| {
        if node_tree_error(node) != MPACK_OK {
            return;
        }
        if nd_type(node) != TYPE_STR {
            flag_tree_error(node.tree, MPACK_ERROR_TYPE);
            return;
        }
        let ptr = node_data_pointer_impl(node, TYPE_STR);
        let len = nd_len(node) as usize;
        if ptr.is_null() {
            return;
        }
        let bytes = unsafe { slice::from_raw_parts(ptr.cast::<u8>(), len) };
        if !reader::check_utf8(bytes) {
            flag_tree_error(node.tree, MPACK_ERROR_TYPE);
        }
    });
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_check_utf8_cstr(node: MpackNode) {
    if node.tree.is_null() {
        return;
    }
    let _ = catch_ffi_panic(|| {
        if node_tree_error(node) != MPACK_OK {
            return;
        }
        if nd_type(node) != TYPE_STR {
            flag_tree_error(node.tree, MPACK_ERROR_TYPE);
            return;
        }
        let ptr = node_data_pointer_impl(node, TYPE_STR);
        let len = nd_len(node) as usize;
        if ptr.is_null() {
            return;
        }
        let bytes = unsafe { slice::from_raw_parts(ptr.cast::<u8>(), len) };
        if !reader::check_utf8_no_null(bytes) {
            flag_tree_error(node.tree, MPACK_ERROR_TYPE);
        }
    });
}

// ── copy / alloc helpers ──────────────────────────────────────────────────────

#[no_mangle]
pub unsafe extern "C" fn mpack_node_copy_data(
    node: MpackNode,
    buffer: *mut c_char,
    bufsize: usize,
) -> usize {
    if node.tree.is_null() {
        return 0;
    }
    match catch_ffi_panic(|| {
        if node_tree_error(node) != MPACK_OK {
            return 0usize;
        }
        let t = nd_type(node);
        if t != TYPE_STR && t != TYPE_BIN && t != TYPE_EXT {
            flag_tree_error(node.tree, MPACK_ERROR_TYPE);
            return 0;
        }
        let len = nd_len(node) as usize;
        if len > bufsize {
            flag_tree_error(node.tree, MPACK_ERROR_TOO_BIG);
            return 0;
        }
        if len > 0 && !buffer.is_null() {
            let src = unsafe {
                let tree_data = tree_data_ptr(node);
                tree_data.add(nd_value(node) as usize)
            };
            unsafe { ptr::copy_nonoverlapping(src, buffer.cast::<u8>(), len) };
        }
        len
    }) {
        Ok(v) => v,
        Err(_) => {
            flag_tree_error(node.tree, MPACK_ERROR_BUG);
            0
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_copy_utf8(
    node: MpackNode,
    buffer: *mut c_char,
    bufsize: usize,
) -> usize {
    if node.tree.is_null() {
        return 0;
    }
    match catch_ffi_panic(|| {
        if node_tree_error(node) != MPACK_OK {
            return 0usize;
        }
        if nd_type(node) != TYPE_STR {
            flag_tree_error(node.tree, MPACK_ERROR_TYPE);
            return 0;
        }
        let len = nd_len(node) as usize;
        if len > bufsize {
            flag_tree_error(node.tree, MPACK_ERROR_TOO_BIG);
            return 0;
        }
        let src_ptr = unsafe {
            let tree_data = tree_data_ptr(node);
            tree_data.add(nd_value(node) as usize)
        };
        let bytes = unsafe { slice::from_raw_parts(src_ptr, len) };
        if !reader::check_utf8(bytes) {
            flag_tree_error(node.tree, MPACK_ERROR_TYPE);
            return 0;
        }
        if len > 0 && !buffer.is_null() {
            unsafe { ptr::copy_nonoverlapping(src_ptr, buffer.cast::<u8>(), len) };
        }
        len
    }) {
        Ok(v) => v,
        Err(_) => {
            flag_tree_error(node.tree, MPACK_ERROR_BUG);
            0
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_copy_cstr(
    node: MpackNode,
    buffer: *mut c_char,
    size: usize,
) {
    if node.tree.is_null() {
        return;
    }
    let _ = catch_ffi_panic(|| {
        if buffer.is_null() {
            assert_fail(b"buffer is NULL\0");
            return;
        }
        if size == 0 {
            assert_fail(b"buffer size is zero; you must have room for at least a null-terminator\0");
            return;
        }
        if node_tree_error(node) != MPACK_OK {
            unsafe { *buffer = 0 };
            return;
        }
        if nd_type(node) != TYPE_STR {
            flag_tree_error(node.tree, MPACK_ERROR_TYPE);
            unsafe { *buffer = 0 };
            return;
        }
        let len = nd_len(node) as usize;
        if len >= size {
            flag_tree_error(node.tree, MPACK_ERROR_TOO_BIG);
            unsafe { *buffer = 0 };
            return;
        }
        let src = unsafe {
            let tree_data = tree_data_ptr(node);
            tree_data.add(nd_value(node) as usize)
        };
        let bytes = unsafe { slice::from_raw_parts(src, len) };
        if bytes.iter().any(|&b| b == 0) {
            flag_tree_error(node.tree, MPACK_ERROR_TYPE);
            unsafe { *buffer = 0 };
            return;
        }
        if len > 0 {
            unsafe { ptr::copy_nonoverlapping(src, buffer.cast::<u8>(), len) };
        }
        unsafe { *buffer.add(len) = 0 };
    });
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_copy_utf8_cstr(
    node: MpackNode,
    buffer: *mut c_char,
    size: usize,
) {
    if node.tree.is_null() {
        return;
    }
    let _ = catch_ffi_panic(|| {
        if buffer.is_null() {
            assert_fail(b"buffer is NULL\0");
            return;
        }
        if size == 0 {
            assert_fail(b"buffer size is zero; you must have room for at least a null-terminator\0");
            return;
        }
        if node_tree_error(node) != MPACK_OK {
            unsafe { *buffer = 0 };
            return;
        }
        if nd_type(node) != TYPE_STR {
            flag_tree_error(node.tree, MPACK_ERROR_TYPE);
            unsafe { *buffer = 0 };
            return;
        }
        let len = nd_len(node) as usize;
        if len >= size {
            flag_tree_error(node.tree, MPACK_ERROR_TOO_BIG);
            unsafe { *buffer = 0 };
            return;
        }
        let src = unsafe {
            let tree_data = tree_data_ptr(node);
            tree_data.add(nd_value(node) as usize)
        };
        let bytes = unsafe { slice::from_raw_parts(src, len) };
        if !reader::check_utf8_no_null(bytes) {
            flag_tree_error(node.tree, MPACK_ERROR_TYPE);
            unsafe { *buffer = 0 };
            return;
        }
        if len > 0 {
            unsafe { ptr::copy_nonoverlapping(src, buffer.cast::<u8>(), len) };
        }
        unsafe { *buffer.add(len) = 0 };
    });
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_data_alloc(
    node: MpackNode,
    maxsize: usize,
) -> *mut c_char {
    if node.tree.is_null() {
        return ptr::null_mut();
    }
    match catch_ffi_panic(|| {
        if node_tree_error(node) != MPACK_OK {
            return ptr::null_mut();
        }
        let t = nd_type(node);
        if t != TYPE_STR && t != TYPE_BIN && t != TYPE_EXT {
            flag_tree_error(node.tree, MPACK_ERROR_TYPE);
            return ptr::null_mut();
        }
        let len = nd_len(node) as usize;
        if len > maxsize {
            flag_tree_error(node.tree, MPACK_ERROR_TOO_BIG);
            return ptr::null_mut();
        }
        let alloc_size = len.max(1);
        let ptr = unsafe { test_malloc(alloc_size).cast::<c_char>() };
        if ptr.is_null() {
            flag_tree_error(node.tree, MPACK_ERROR_MEMORY);
            return ptr::null_mut();
        }
        if len > 0 {
            let src = unsafe {
                let tree_data = tree_data_ptr(node);
                tree_data.add(nd_value(node) as usize)
            };
            unsafe { ptr::copy_nonoverlapping(src, ptr.cast::<u8>(), len) };
        }
        ptr
    }) {
        Ok(p) => p,
        Err(_) => {
            flag_tree_error(node.tree, MPACK_ERROR_BUG);
            ptr::null_mut()
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_cstr_alloc(
    node: MpackNode,
    maxsize: usize,
) -> *mut c_char {
    if node.tree.is_null() {
        return ptr::null_mut();
    }
    match catch_ffi_panic(|| {
        if node_tree_error(node) != MPACK_OK {
            return ptr::null_mut();
        }
        if maxsize < 1 {
            break_hit(b"maxlen is zero; you must have room for at least a null-terminator\0");
            flag_tree_error(node.tree, MPACK_ERROR_BUG);
            return ptr::null_mut();
        }
        if nd_type(node) != TYPE_STR {
            flag_tree_error(node.tree, MPACK_ERROR_TYPE);
            return ptr::null_mut();
        }
        let len = nd_len(node) as usize;
        if len >= maxsize {
            flag_tree_error(node.tree, MPACK_ERROR_TOO_BIG);
            return ptr::null_mut();
        }
        let src_ptr = unsafe {
            let tree_data = tree_data_ptr(node);
            tree_data.add(nd_value(node) as usize)
        };
        let bytes = unsafe { slice::from_raw_parts(src_ptr, len) };
        // Check for interior NUL
        if bytes.iter().any(|&b| b == 0) {
            flag_tree_error(node.tree, MPACK_ERROR_TYPE);
            return ptr::null_mut();
        }
        let alloc_size = len + 1;
        let ptr = unsafe { test_malloc(alloc_size).cast::<c_char>() };
        if ptr.is_null() {
            flag_tree_error(node.tree, MPACK_ERROR_MEMORY);
            return ptr::null_mut();
        }
        if len > 0 {
            unsafe { ptr::copy_nonoverlapping(src_ptr, ptr.cast::<u8>(), len) };
        }
        unsafe { *ptr.add(len) = 0 };
        ptr
    }) {
        Ok(p) => p,
        Err(_) => {
            flag_tree_error(node.tree, MPACK_ERROR_BUG);
            ptr::null_mut()
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_utf8_cstr_alloc(
    node: MpackNode,
    maxsize: usize,
) -> *mut c_char {
    if node.tree.is_null() {
        return ptr::null_mut();
    }
    match catch_ffi_panic(|| {
        if node_tree_error(node) != MPACK_OK {
            return ptr::null_mut();
        }
        if maxsize < 1 {
            break_hit(b"maxlen is zero; you must have room for at least a null-terminator\0");
            flag_tree_error(node.tree, MPACK_ERROR_BUG);
            return ptr::null_mut();
        }
        if nd_type(node) != TYPE_STR {
            flag_tree_error(node.tree, MPACK_ERROR_TYPE);
            return ptr::null_mut();
        }
        let len = nd_len(node) as usize;
        if len >= maxsize {
            flag_tree_error(node.tree, MPACK_ERROR_TOO_BIG);
            return ptr::null_mut();
        }
        let src_ptr = unsafe {
            let tree_data = tree_data_ptr(node);
            tree_data.add(nd_value(node) as usize)
        };
        let bytes = unsafe { slice::from_raw_parts(src_ptr, len) };
        if !reader::check_utf8_no_null(bytes) {
            flag_tree_error(node.tree, MPACK_ERROR_TYPE);
            return ptr::null_mut();
        }
        let alloc_size = len + 1;
        let ptr = unsafe { test_malloc(alloc_size).cast::<c_char>() };
        if ptr.is_null() {
            flag_tree_error(node.tree, MPACK_ERROR_MEMORY);
            return ptr::null_mut();
        }
        if len > 0 {
            unsafe { ptr::copy_nonoverlapping(src_ptr, ptr.cast::<u8>(), len) };
        }
        unsafe { *ptr.add(len) = 0 };
        ptr
    }) {
        Ok(p) => p,
        Err(_) => {
            flag_tree_error(node.tree, MPACK_ERROR_BUG);
            ptr::null_mut()
        }
    }
}

// ── enum lookup ───────────────────────────────────────────────────────────────

fn node_enum_impl(
    node: MpackNode,
    strings: *const *const c_char,
    count: usize,
    optional: bool,
) -> usize {
    if node_tree_error(node) != MPACK_OK {
        return count;
    }
    // C: only strings are recognized; non-str → count (optional: no error;
    // required: type error via the shared optional→required wrapper).
    if nd_type(node) != TYPE_STR {
        if !optional {
            flag_tree_error(node.tree, MPACK_ERROR_TYPE);
        }
        return count;
    }
    let len = nd_len(node) as usize;
    let src = {
        let tree_data = tree_data_ptr(node);
        if tree_data.is_null() {
            return count;
        }
        unsafe { slice::from_raw_parts(tree_data.add(nd_value(node) as usize), len) }
    };
    for i in 0..count {
        let s_ptr = unsafe { *strings.add(i) };
        if s_ptr.is_null() {
            continue;
        }
        let s_bytes = unsafe { CStr::from_ptr(s_ptr).to_bytes() };
        if s_bytes == src {
            return i;
        }
    }
    if !optional {
        flag_tree_error(node.tree, MPACK_ERROR_TYPE);
    }
    count
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_enum(
    node: MpackNode,
    strings: *const *const c_char,
    count: usize,
) -> usize {
    if node.tree.is_null() {
        return count;
    }
    match catch_ffi_panic(|| node_enum_impl(node, strings, count, false)) {
        Ok(v) => v,
        Err(_) => {
            flag_tree_error(node.tree, MPACK_ERROR_BUG);
            count
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_enum_optional(
    node: MpackNode,
    strings: *const *const c_char,
    count: usize,
) -> usize {
    if node.tree.is_null() {
        return count;
    }
    match catch_ffi_panic(|| node_enum_impl(node, strings, count, true)) {
        Ok(v) => v,
        Err(_) => {
            flag_tree_error(node.tree, MPACK_ERROR_BUG);
            count
        }
    }
}

// ── compound: array ───────────────────────────────────────────────────────────

#[no_mangle]
pub unsafe extern "C" fn mpack_node_array_length(node: MpackNode) -> usize {
    if node.tree.is_null() {
        return 0;
    }
    match catch_ffi_panic(|| {
        if node_tree_error(node) != MPACK_OK {
            return 0usize;
        }
        if nd_type(node) != TYPE_ARRAY {
            flag_tree_error(node.tree, MPACK_ERROR_TYPE);
            return 0;
        }
        nd_len(node) as usize
    }) {
        Ok(v) => v,
        Err(_) => {
            flag_tree_error(node.tree, MPACK_ERROR_BUG);
            0
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_array_at(node: MpackNode, index: usize) -> MpackNode {
    if node.tree.is_null() {
        return MpackNode::null();
    }
    match catch_ffi_panic(|| {
        if node_tree_error(node) != MPACK_OK {
            return nil_node_for(node.tree);
        }
        if nd_type(node) != TYPE_ARRAY {
            flag_tree_error(node.tree, MPACK_ERROR_TYPE);
            return nil_node_for(node.tree);
        }
        let count = nd_len(node) as usize;
        if index >= count {
            flag_tree_error(node.tree, MPACK_ERROR_DATA);
            return nil_node_for(node.tree);
        }
        let children = nd_value(node) as *mut MpackNodeData;
        if children.is_null() {
            flag_tree_error(node.tree, MPACK_ERROR_BUG);
            return nil_node_for(node.tree);
        }
        MpackNode {
            data: unsafe { children.add(index) },
            tree: node.tree,
        }
    }) {
        Ok(n) => n,
        Err(_) => {
            flag_tree_error(node.tree, MPACK_ERROR_BUG);
            nil_node_for(node.tree)
        }
    }
}

// ── compound: map ─────────────────────────────────────────────────────────────

#[no_mangle]
pub unsafe extern "C" fn mpack_node_map_count(node: MpackNode) -> usize {
    if node.tree.is_null() {
        return 0;
    }
    match catch_ffi_panic(|| {
        if node_tree_error(node) != MPACK_OK {
            return 0usize;
        }
        if nd_type(node) != TYPE_MAP {
            flag_tree_error(node.tree, MPACK_ERROR_TYPE);
            return 0;
        }
        nd_len(node) as usize
    }) {
        Ok(v) => v,
        Err(_) => {
            flag_tree_error(node.tree, MPACK_ERROR_BUG);
            0
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_map_key_at(node: MpackNode, index: usize) -> MpackNode {
    if node.tree.is_null() {
        return MpackNode::null();
    }
    match catch_ffi_panic(|| {
        if node_tree_error(node) != MPACK_OK {
            return nil_node_for(node.tree);
        }
        if nd_type(node) != TYPE_MAP {
            flag_tree_error(node.tree, MPACK_ERROR_TYPE);
            return nil_node_for(node.tree);
        }
        if index >= nd_len(node) as usize {
            flag_tree_error(node.tree, MPACK_ERROR_DATA);
            return nil_node_for(node.tree);
        }
        let children = nd_value(node) as *mut MpackNodeData;
        if children.is_null() {
            flag_tree_error(node.tree, MPACK_ERROR_BUG);
            return nil_node_for(node.tree);
        }
        MpackNode {
            data: unsafe { children.add(index * 2) },
            tree: node.tree,
        }
    }) {
        Ok(n) => n,
        Err(_) => {
            flag_tree_error(node.tree, MPACK_ERROR_BUG);
            nil_node_for(node.tree)
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_map_value_at(node: MpackNode, index: usize) -> MpackNode {
    if node.tree.is_null() {
        return MpackNode::null();
    }
    match catch_ffi_panic(|| {
        if node_tree_error(node) != MPACK_OK {
            return nil_node_for(node.tree);
        }
        if nd_type(node) != TYPE_MAP {
            flag_tree_error(node.tree, MPACK_ERROR_TYPE);
            return nil_node_for(node.tree);
        }
        if index >= nd_len(node) as usize {
            flag_tree_error(node.tree, MPACK_ERROR_DATA);
            return nil_node_for(node.tree);
        }
        let children = nd_value(node) as *mut MpackNodeData;
        if children.is_null() {
            flag_tree_error(node.tree, MPACK_ERROR_BUG);
            return nil_node_for(node.tree);
        }
        MpackNode {
            data: unsafe { children.add(index * 2 + 1) },
            tree: node.tree,
        }
    }) {
        Ok(n) => n,
        Err(_) => {
            flag_tree_error(node.tree, MPACK_ERROR_BUG);
            nil_node_for(node.tree)
        }
    }
}

// ── map search helpers ────────────────────────────────────────────────────────

/// Search a map for a value whose key matches `key_matches`. Returns the
/// ABI pointer to the value node, or null if not found.
fn map_search(
    node: MpackNode,
    key_matches: impl Fn(*const MpackNodeData, *const u8) -> bool,
) -> *mut MpackNodeData {
    if nd_type(node) != TYPE_MAP {
        flag_tree_error(node.tree, MPACK_ERROR_TYPE);
        return ptr::null_mut();
    }
    let count = nd_len(node) as usize;
    if count == 0 {
        return ptr::null_mut();
    }
    let children = nd_value(node) as *mut MpackNodeData;
    if children.is_null() {
        return ptr::null_mut();
    }
    let tree_data = tree_data_ptr(node);
    let mut found = ptr::null_mut::<MpackNodeData>();
    let mut duplicates = 0usize;
    for i in 0..count {
        let key_ptr = unsafe { children.add(i * 2) as *const MpackNodeData };
        if unsafe { key_matches(key_ptr, tree_data) } {
            if found.is_null() {
                found = unsafe { children.add(i * 2 + 1) };
            } else {
                duplicates += 1;
            }
        }
    }
    if duplicates > 0 {
        flag_tree_error(node.tree, MPACK_ERROR_DATA);
        return ptr::null_mut();
    }
    found
}

fn key_is_int(ptr: *const MpackNodeData, _data: *const u8, key: i64) -> bool {
    let nd = unsafe { &*ptr };
    match nd.type_ {
        TYPE_INT => nd.value as i64 == key,
        TYPE_UINT => {
            let v = nd.value;
            v <= i64::MAX as u64 && v as i64 == key
        }
        _ => false,
    }
}

fn key_is_uint(ptr: *const MpackNodeData, _data: *const u8, key: u64) -> bool {
    let nd = unsafe { &*ptr };
    match nd.type_ {
        TYPE_UINT => nd.value == key,
        TYPE_INT => {
            let v = nd.value as i64;
            v >= 0 && v as u64 == key
        }
        _ => false,
    }
}

fn key_is_str(ptr: *const MpackNodeData, data: *const u8, key: &[u8]) -> bool {
    let nd = unsafe { &*ptr };
    if nd.type_ != TYPE_STR {
        return false;
    }
    let len = nd.len as usize;
    if len != key.len() {
        return false;
    }
    if len == 0 {
        return true;
    }
    if data.is_null() {
        return false;
    }
    let s = unsafe { slice::from_raw_parts(data.add(nd.value as usize), len) };
    s == key
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_map_int(node: MpackNode, num: i64) -> MpackNode {
    if node.tree.is_null() {
        return MpackNode::null();
    }
    match catch_ffi_panic(|| {
        if node_tree_error(node) != MPACK_OK {
            return nil_node_for(node.tree);
        }
        let val_ptr = map_search(node, |k, d| key_is_int(k, d, num));
        if val_ptr.is_null() {
            if node_tree_error(node) == MPACK_OK {
                flag_tree_error(node.tree, MPACK_ERROR_DATA);
            }
            nil_node_for(node.tree)
        } else {
            MpackNode {
                data: val_ptr,
                tree: node.tree,
            }
        }
    }) {
        Ok(n) => n,
        Err(_) => {
            flag_tree_error(node.tree, MPACK_ERROR_BUG);
            nil_node_for(node.tree)
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_map_uint(node: MpackNode, num: u64) -> MpackNode {
    if node.tree.is_null() {
        return MpackNode::null();
    }
    match catch_ffi_panic(|| {
        if node_tree_error(node) != MPACK_OK {
            return nil_node_for(node.tree);
        }
        let val_ptr = map_search(node, |k, d| key_is_uint(k, d, num));
        if val_ptr.is_null() {
            if node_tree_error(node) == MPACK_OK {
                flag_tree_error(node.tree, MPACK_ERROR_DATA);
            }
            nil_node_for(node.tree)
        } else {
            MpackNode {
                data: val_ptr,
                tree: node.tree,
            }
        }
    }) {
        Ok(n) => n,
        Err(_) => {
            flag_tree_error(node.tree, MPACK_ERROR_BUG);
            nil_node_for(node.tree)
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_map_str(
    node: MpackNode,
    str_: *const c_char,
    length: usize,
) -> MpackNode {
    if node.tree.is_null() {
        return MpackNode::null();
    }
    match catch_ffi_panic(|| {
        if node_tree_error(node) != MPACK_OK {
            return nil_node_for(node.tree);
        }
        let key: &[u8] = if str_.is_null() || length == 0 {
            &[][..]
        } else {
            unsafe { slice::from_raw_parts(str_.cast::<u8>(), length) }
        };
        let val_ptr = map_search(node, |k, d| key_is_str(k, d, key));
        if val_ptr.is_null() {
            if node_tree_error(node) == MPACK_OK {
                flag_tree_error(node.tree, MPACK_ERROR_DATA);
            }
            nil_node_for(node.tree)
        } else {
            MpackNode {
                data: val_ptr,
                tree: node.tree,
            }
        }
    }) {
        Ok(n) => n,
        Err(_) => {
            flag_tree_error(node.tree, MPACK_ERROR_BUG);
            nil_node_for(node.tree)
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_map_cstr(
    node: MpackNode,
    cstr: *const c_char,
) -> MpackNode {
    if node.tree.is_null() {
        return MpackNode::null();
    }
    if cstr.is_null() {
        flag_tree_error(node.tree, MPACK_ERROR_BUG);
        return nil_node_for(node.tree);
    }
    let key = unsafe { CStr::from_ptr(cstr).to_bytes() };
    match catch_ffi_panic(|| {
        if node_tree_error(node) != MPACK_OK {
            return nil_node_for(node.tree);
        }
        let val_ptr = map_search(node, |k, d| key_is_str(k, d, key));
        if val_ptr.is_null() {
            if node_tree_error(node) == MPACK_OK {
                flag_tree_error(node.tree, MPACK_ERROR_DATA);
            }
            nil_node_for(node.tree)
        } else {
            MpackNode {
                data: val_ptr,
                tree: node.tree,
            }
        }
    }) {
        Ok(n) => n,
        Err(_) => {
            flag_tree_error(node.tree, MPACK_ERROR_BUG);
            nil_node_for(node.tree)
        }
    }
}

// ── optional map lookups (return missing_node instead of error on miss) ───────

#[no_mangle]
pub unsafe extern "C" fn mpack_node_map_int_optional(node: MpackNode, num: i64) -> MpackNode {
    if node.tree.is_null() {
        return MpackNode::null();
    }
    match catch_ffi_panic(|| {
        if node_tree_error(node) != MPACK_OK {
            return nil_node_for(node.tree);
        }
        let val_ptr = map_search(node, |k, d| key_is_int(k, d, num));
        if val_ptr.is_null() {
            if node_tree_error(node) == MPACK_OK {
                // No error — return missing
                missing_node_for(node.tree)
            } else {
                nil_node_for(node.tree)
            }
        } else {
            MpackNode {
                data: val_ptr,
                tree: node.tree,
            }
        }
    }) {
        Ok(n) => n,
        Err(_) => {
            flag_tree_error(node.tree, MPACK_ERROR_BUG);
            nil_node_for(node.tree)
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_map_uint_optional(node: MpackNode, num: u64) -> MpackNode {
    if node.tree.is_null() {
        return MpackNode::null();
    }
    match catch_ffi_panic(|| {
        if node_tree_error(node) != MPACK_OK {
            return nil_node_for(node.tree);
        }
        let val_ptr = map_search(node, |k, d| key_is_uint(k, d, num));
        if val_ptr.is_null() {
            if node_tree_error(node) == MPACK_OK {
                missing_node_for(node.tree)
            } else {
                nil_node_for(node.tree)
            }
        } else {
            MpackNode {
                data: val_ptr,
                tree: node.tree,
            }
        }
    }) {
        Ok(n) => n,
        Err(_) => {
            flag_tree_error(node.tree, MPACK_ERROR_BUG);
            nil_node_for(node.tree)
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_map_str_optional(
    node: MpackNode,
    str_: *const c_char,
    length: usize,
) -> MpackNode {
    if node.tree.is_null() {
        return MpackNode::null();
    }
    match catch_ffi_panic(|| {
        if node_tree_error(node) != MPACK_OK {
            return nil_node_for(node.tree);
        }
        let key: &[u8] = if str_.is_null() || length == 0 {
            &[][..]
        } else {
            unsafe { slice::from_raw_parts(str_.cast::<u8>(), length) }
        };
        let val_ptr = map_search(node, |k, d| key_is_str(k, d, key));
        if val_ptr.is_null() {
            if node_tree_error(node) == MPACK_OK {
                missing_node_for(node.tree)
            } else {
                nil_node_for(node.tree)
            }
        } else {
            MpackNode {
                data: val_ptr,
                tree: node.tree,
            }
        }
    }) {
        Ok(n) => n,
        Err(_) => {
            flag_tree_error(node.tree, MPACK_ERROR_BUG);
            nil_node_for(node.tree)
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_map_cstr_optional(
    node: MpackNode,
    cstr: *const c_char,
) -> MpackNode {
    if node.tree.is_null() {
        return MpackNode::null();
    }
    if cstr.is_null() {
        flag_tree_error(node.tree, MPACK_ERROR_BUG);
        return nil_node_for(node.tree);
    }
    let key = unsafe { CStr::from_ptr(cstr).to_bytes() };
    match catch_ffi_panic(|| {
        if node_tree_error(node) != MPACK_OK {
            return nil_node_for(node.tree);
        }
        let val_ptr = map_search(node, |k, d| key_is_str(k, d, key));
        if val_ptr.is_null() {
            if node_tree_error(node) == MPACK_OK {
                missing_node_for(node.tree)
            } else {
                nil_node_for(node.tree)
            }
        } else {
            MpackNode {
                data: val_ptr,
                tree: node.tree,
            }
        }
    }) {
        Ok(n) => n,
        Err(_) => {
            flag_tree_error(node.tree, MPACK_ERROR_BUG);
            nil_node_for(node.tree)
        }
    }
}

// ── contains checks ───────────────────────────────────────────────────────────

#[no_mangle]
pub unsafe extern "C" fn mpack_node_map_contains_int(node: MpackNode, num: i64) -> bool {
    if node.tree.is_null() {
        return false;
    }
    match catch_ffi_panic(|| {
        if node_tree_error(node) != MPACK_OK {
            return false;
        }
        if nd_type(node) != TYPE_MAP {
            flag_tree_error(node.tree, MPACK_ERROR_TYPE);
            return false;
        }
        !map_search(node, |k, d| key_is_int(k, d, num)).is_null()
            && node_tree_error(node) == MPACK_OK
    }) {
        Ok(v) => v,
        Err(_) => {
            flag_tree_error(node.tree, MPACK_ERROR_BUG);
            false
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_map_contains_uint(node: MpackNode, num: u64) -> bool {
    if node.tree.is_null() {
        return false;
    }
    match catch_ffi_panic(|| {
        if node_tree_error(node) != MPACK_OK {
            return false;
        }
        if nd_type(node) != TYPE_MAP {
            flag_tree_error(node.tree, MPACK_ERROR_TYPE);
            return false;
        }
        !map_search(node, |k, d| key_is_uint(k, d, num)).is_null()
            && node_tree_error(node) == MPACK_OK
    }) {
        Ok(v) => v,
        Err(_) => {
            flag_tree_error(node.tree, MPACK_ERROR_BUG);
            false
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_map_contains_str(
    node: MpackNode,
    str_: *const c_char,
    length: usize,
) -> bool {
    if node.tree.is_null() {
        return false;
    }
    match catch_ffi_panic(|| {
        if node_tree_error(node) != MPACK_OK {
            return false;
        }
        if nd_type(node) != TYPE_MAP {
            flag_tree_error(node.tree, MPACK_ERROR_TYPE);
            return false;
        }
        let key: &[u8] = if str_.is_null() || length == 0 {
            &[][..]
        } else {
            unsafe { slice::from_raw_parts(str_.cast::<u8>(), length) }
        };
        !map_search(node, |k, d| key_is_str(k, d, key)).is_null()
            && node_tree_error(node) == MPACK_OK
    }) {
        Ok(v) => v,
        Err(_) => {
            flag_tree_error(node.tree, MPACK_ERROR_BUG);
            false
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn mpack_node_map_contains_cstr(
    node: MpackNode,
    cstr: *const c_char,
) -> bool {
    if node.tree.is_null() || cstr.is_null() {
        return false;
    }
    let key = unsafe { CStr::from_ptr(cstr).to_bytes() };
    match catch_ffi_panic(|| {
        if node_tree_error(node) != MPACK_OK {
            return false;
        }
        if nd_type(node) != TYPE_MAP {
            flag_tree_error(node.tree, MPACK_ERROR_TYPE);
            return false;
        }
        !map_search(node, |k, d| key_is_str(k, d, key)).is_null()
            && node_tree_error(node) == MPACK_OK
    }) {
        Ok(v) => v,
        Err(_) => {
            flag_tree_error(node.tree, MPACK_ERROR_BUG);
            false
        }
    }
}

// ── flag_error / missing_node ─────────────────────────────────────────────────

/// Flags a sticky error on the node's tree.
#[no_mangle]
pub unsafe extern "C" fn mpack_node_flag_error(node: MpackNode, error: MpackError) {
    if node.tree.is_null() {
        return;
    }
    flag_tree_error(node.tree, error);
}

/// Expects a missing node; flags type error otherwise.
#[no_mangle]
pub unsafe extern "C" fn mpack_node_missing(node: MpackNode) {
    if node.tree.is_null() {
        return;
    }
    let _ = catch_ffi_panic(|| {
        if node_tree_error(node) != MPACK_OK {
            return;
        }
        if nd_type(node) != TYPE_MISSING {
            flag_tree_error(node.tree, MPACK_ERROR_TYPE);
        }
    });
}

// ── print functions ───────────────────────────────────────────────────────────

/// Pretty-prints the subtree rooted at `node` into a NUL-terminated buffer.
///
/// # Safety
/// `buffer` must be writable for `buffer_size` bytes when non-null.
#[no_mangle]
pub unsafe extern "C" fn mpack_node_print_to_buffer(
    node: MpackNode,
    buffer: *mut c_char,
    buffer_size: usize,
) {
    if buffer.is_null() || buffer_size == 0 {
        return;
    }
    let _ = catch_ffi_panic(|| {
        let mut output: Vec<u8> = Vec::new();
        if node.tree.is_null() || node.data.is_null() || node_tree_error(node) != MPACK_OK {
            let _ = write!(output, "<mpack node error>");
        } else {
            let tree_data = tree_data_ptr(node);
            print_node_to_vec(node.data, tree_data, &mut output, 0);
        }
        let copy = output.len().min(buffer_size.saturating_sub(1));
        if copy > 0 {
            unsafe { ptr::copy_nonoverlapping(output.as_ptr(), buffer.cast::<u8>(), copy) };
        }
        unsafe {
            *buffer.add(copy) = 0;
            *buffer.add(buffer_size - 1) = 0;
        }
    });
}

/// Pretty-prints the subtree rooted at `node` to a C `FILE*`.
///
/// # Safety
/// `file` must be a live `FILE*`.
#[no_mangle]
pub unsafe extern "C" fn mpack_node_print_to_file(node: MpackNode, file: *mut c_void) {
    if file.is_null() {
        return;
    }
    let _ = catch_ffi_panic(|| {
        let mut output: Vec<u8> = Vec::new();
        if node.tree.is_null() || node.data.is_null() || node_tree_error(node) != MPACK_OK {
            let _ = write!(output, "<mpack node error>");
        } else {
            let tree_data = tree_data_ptr(node);
            print_node_to_vec(node.data, tree_data, &mut output, 0);
        }
        output.push(b'\n');
        if !output.is_empty() {
            unsafe { fwrite(output.as_ptr().cast(), 1, output.len(), file) };
        }
    });
}

fn print_node_to_vec(
    node_data: *const MpackNodeData,
    tree_data: *const u8,
    output: &mut Vec<u8>,
    depth: usize,
) {
    // Iterative tree walk to avoid stack overflow on deep trees.
    enum Frame {
        Array {
            children: *const MpackNodeData,
            total: u32,
            next: u32,
            depth: usize,
        },
        Map {
            children: *const MpackNodeData,
            pairs: u32,
            pair: u32,
            is_key: bool,
            depth: usize,
        },
    }

    let mut stack: Vec<Frame> = Vec::new();
    let mut current = node_data;
    let mut current_depth = depth;

    loop {
        if current.is_null() {
            break;
        }
        let nd = unsafe { &*current };
        match nd.type_ {
            TYPE_NIL => {
                let _ = write!(output, "null");
            }
            TYPE_MISSING => {
                let _ = write!(output, "<missing>");
            }
            TYPE_BOOL => {
                let _ = write!(output, "{}", if nd.value != 0 { "true" } else { "false" });
            }
            TYPE_INT => {
                let _ = write!(output, "{}", nd.value as i64);
            }
            TYPE_UINT => {
                let _ = write!(output, "{}", nd.value);
            }
            TYPE_FLOAT => {
                let _ = write!(output, "{:.6}", f32::from_bits(nd.value as u32));
            }
            TYPE_DOUBLE => {
                let _ = write!(output, "{:.6}", f64::from_bits(nd.value));
            }
            TYPE_STR => {
                let _ = write!(output, "\"");
                if !tree_data.is_null() && nd.len > 0 {
                    let bytes = unsafe {
                        slice::from_raw_parts(tree_data.add(nd.value as usize), nd.len as usize)
                    };
                    for &b in bytes {
                        match b {
                            b'\n' => {
                                let _ = write!(output, "\\n");
                            }
                            b'\\' => {
                                let _ = write!(output, "\\\\");
                            }
                            b'"' => {
                                let _ = write!(output, "\\\"");
                            }
                            _ => output.push(b),
                        }
                    }
                }
                let _ = write!(output, "\"");
            }
            TYPE_BIN => {
                let len = nd.len;
                let _ = write!(output, "<binary data of length {len}");
                if len > 0 && !tree_data.is_null() {
                    let take = (len as usize).min(PRINT_BYTE_COUNT);
                    let bytes = unsafe {
                        slice::from_raw_parts(tree_data.add(nd.value as usize), take)
                    };
                    let _ = write!(output, ": ");
                    for &b in bytes {
                        let _ = write!(output, "{b:02x}");
                    }
                    if len as usize > take {
                        let _ = write!(output, "...");
                    }
                }
                let _ = write!(output, ">");
            }
            TYPE_EXT => {
                let exttype = if !tree_data.is_null() && nd.value > 0 {
                    unsafe { *tree_data.add(nd.value as usize - 1) as i8 }
                } else {
                    0
                };
                let len = nd.len;
                let _ = write!(output, "<ext data of type {exttype} and length {len}");
                if len > 0 && !tree_data.is_null() {
                    let take = (len as usize).min(PRINT_BYTE_COUNT);
                    let bytes = unsafe {
                        slice::from_raw_parts(tree_data.add(nd.value as usize), take)
                    };
                    let _ = write!(output, ": ");
                    for &b in bytes {
                        let _ = write!(output, "{b:02x}");
                    }
                    if len as usize > take {
                        let _ = write!(output, "...");
                    }
                }
                let _ = write!(output, ">");
            }
            TYPE_ARRAY => {
                let count = nd.len;
                let children = nd.value as *const MpackNodeData;
                let _ = write!(output, "[\n");
                if count == 0 {
                    print_indent(output, current_depth);
                    let _ = write!(output, "]");
                } else {
                    print_indent(output, current_depth + 1);
                    stack.push(Frame::Array {
                        children,
                        total: count,
                        next: 0,
                        depth: current_depth,
                    });
                    current = if children.is_null() {
                        ptr::null()
                    } else {
                        children
                    };
                    current_depth += 1;
                    continue;
                }
            }
            TYPE_MAP => {
                let pairs = nd.len;
                let children = nd.value as *const MpackNodeData;
                let _ = write!(output, "{{\n");
                if pairs == 0 {
                    print_indent(output, current_depth);
                    let _ = write!(output, "}}");
                } else {
                    print_indent(output, current_depth + 1);
                    stack.push(Frame::Map {
                        children,
                        pairs,
                        pair: 0,
                        is_key: true,
                        depth: current_depth,
                    });
                    current = if children.is_null() {
                        ptr::null()
                    } else {
                        children // first key
                    };
                    current_depth += 1;
                    continue;
                }
            }
            _ => {
                let _ = write!(output, "<unknown>");
            }
        }

        // After processing a scalar or closing a compound, advance frames
        current = ptr::null();
        loop {
            let Some(frame) = stack.last_mut() else {
                return;
            };
            match frame {
                Frame::Array {
                    children,
                    total,
                    next,
                    depth,
                } => {
                    *next += 1;
                    if *next < *total {
                        let _ = write!(output, ",\n");
                        let d = *depth;
                        let n = *next;
                        let ch = *children;
                        print_indent(output, d + 1);
                        current = if ch.is_null() {
                            ptr::null()
                        } else {
                            unsafe { ch.add(n as usize) }
                        };
                        current_depth = d + 1;
                        break;
                    } else {
                        let _ = write!(output, "\n");
                        let d = *depth;
                        stack.pop();
                        print_indent(output, d);
                        let _ = write!(output, "]");
                        continue;
                    }
                }
                Frame::Map {
                    children,
                    pairs,
                    pair,
                    is_key,
                    depth,
                } => {
                    if *is_key {
                        // Just printed a key, now print ": " and go to value
                        let _ = write!(output, ": ");
                        *is_key = false;
                        let p = *pair;
                        let ch = *children;
                        let d = *depth;
                        current = if ch.is_null() {
                            ptr::null()
                        } else {
                            unsafe { ch.add(p as usize * 2 + 1) }
                        };
                        current_depth = d + 1;
                        break;
                    } else {
                        // Just printed a value
                        *pair += 1;
                        if *pair < *pairs {
                            let _ = write!(output, ",\n");
                            let d = *depth;
                            let p = *pair;
                            let ch = *children;
                            print_indent(output, d + 1);
                            *is_key = true;
                            current = if ch.is_null() {
                                ptr::null()
                            } else {
                                unsafe { ch.add(p as usize * 2) }
                            };
                            current_depth = d + 1;
                            break;
                        } else {
                            let _ = write!(output, "\n");
                            let d = *depth;
                            stack.pop();
                            print_indent(output, d);
                            let _ = write!(output, "}}");
                            continue;
                        }
                    }
                }
            }
        }
    }
}

fn print_indent(output: &mut Vec<u8>, depth: usize) {
    for _ in 0..depth {
        let _ = write!(output, "    ");
    }
}
