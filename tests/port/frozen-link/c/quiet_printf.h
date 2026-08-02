/*
 * Force-included only under frozen-link `--soft-continue` (debug).
 *
 * Parity / acceptance builds must NOT include this header.
 * Redirect printf so soft-continued assertion spam does not dominate runtime.
 * Does not change pass/fail counters; still not a substitute for parity.
 *
 * Include <stdio.h> before remapping printf/vprintf so libc declarations are
 * processed under their real names (include guards), then redirect call sites.
 */
#ifndef MPACK_QUIET_PRINTF_H
#define MPACK_QUIET_PRINTF_H 1

#include <stdarg.h>
#include <stdio.h>

int mpack_quiet_printf(const char* format, ...);
int mpack_quiet_vprintf(const char* format, va_list args);

#undef printf
#undef vprintf
#define printf mpack_quiet_printf
#define vprintf mpack_quiet_vprintf

#endif
