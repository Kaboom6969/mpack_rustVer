/*
 * Force-included only under frozen-link `--soft-continue` (debug).
 *
 * Parity / acceptance builds must NOT include this header: the frozen suite
 * hardcodes TEST_EARLY_EXIT which calls abort() on failure, matching upstream.
 *
 * Soft-continue redirects abort to a returning function so fall-through after
 * TEST_EARLY_EXIT remains defined under GCC's noreturn assumptions for libc
 * abort, allowing a full failure summary. The runner still forwards the suite
 * exit / failure count — this is never a fake green.
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
