/*
 * Force-included under --everything / --default-config (alias) only.
 *
 * The frozen suite hardcodes TEST_EARLY_EXIT which calls abort() on failure.
 * GCC treats abort as noreturn, so wrapping the libc symbol still leaves
 * optimized control flow that never decrements loop counters. Redirecting
 * abort to an ordinary function restores defined fall-through for stubs.
 *
 * Include <stdlib.h> BEFORE defining abort→mpack_soft_abort. Otherwise the
 * libc `void abort(void) __attribute__((noreturn))` declaration is rewritten
 * into a noreturn declaration of mpack_soft_abort; the compiler then omits
 * epilogues after abort() call sites, and a real return triggers
 * "stack smashing detected".
 */
#ifndef MPACK_SOFT_ABORT_H
#define MPACK_SOFT_ABORT_H 1

#include <stdlib.h>

void mpack_soft_abort(void);

#undef abort
#define abort mpack_soft_abort

#endif
