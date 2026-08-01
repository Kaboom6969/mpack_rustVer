/*
 * Force-included under --default-config only.
 *
 * The frozen suite hardcodes TEST_EARLY_EXIT which calls abort() on failure.
 * GCC treats abort as noreturn, so wrapping the libc symbol still leaves
 * optimized control flow that never decrements loop counters. Redirecting
 * abort to an ordinary function restores defined fall-through for stubs.
 */
#ifndef MPACK_SOFT_ABORT_H
#define MPACK_SOFT_ABORT_H 1

void mpack_soft_abort(void);
#undef abort
#define abort mpack_soft_abort

#endif
