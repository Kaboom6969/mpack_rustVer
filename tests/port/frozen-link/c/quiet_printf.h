/*
 * Force-included under --everything / --default-config only.
 * Redirect printf so soft-continued assertion spam does not dominate runtime.
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
