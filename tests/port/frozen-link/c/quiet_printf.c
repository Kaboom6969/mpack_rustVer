/*
 * Speed filter for the default-config stub gate.
 *
 * Soft-continued TEST_EARLY_EXIT walks huge compound-size loops that print on
 * every failure. Swallow ordinary printf/vprintf traffic but keep the final
 * summary line from the frozen suite.
 */
#define _GNU_SOURCE
#include <stdarg.h>
#include <stdbool.h>
#include <stdio.h>
#include <string.h>

static bool keep_message(const char* format) {
    return format != NULL && strstr(format, "Unit testing complete") != NULL;
}

int printf(const char* format, ...) {
    if (keep_message(format)) {
        va_list args;
        va_start(args, format);
        int result = vfprintf(stdout, format, args);
        va_end(args);
        return result;
    }
    return 0;
}

int vprintf(const char* format, va_list args) {
    if (keep_message(format)) {
        return vfprintf(stdout, format, args);
    }
    return 0;
}
