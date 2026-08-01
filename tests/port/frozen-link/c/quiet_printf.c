/*
 * Speed filter for the everything / default-config stub gate.
 *
 * Soft-continued TEST_EARLY_EXIT walks huge compound-size loops that print on
 * every failure. Swallow ordinary printf/vprintf traffic but keep the final
 * summary line from the frozen suite. Suite sources see these via quiet_printf.h
 * force-include (#define printf mpack_quiet_printf).
 */
#include <stdarg.h>
#include <stdbool.h>
#include <stdio.h>
#include <string.h>

static bool keep_message(const char* format) {
    return format != NULL && strstr(format, "Unit testing complete") != NULL;
}

int mpack_quiet_printf(const char* format, ...) {
    if (keep_message(format)) {
        va_list args;
        va_start(args, format);
        int result = vfprintf(stdout, format, args);
        va_end(args);
        return result;
    }
    return 0;
}

int mpack_quiet_vprintf(const char* format, va_list args) {
    if (keep_message(format)) {
        return vfprintf(stdout, format, args);
    }
    return 0;
}
