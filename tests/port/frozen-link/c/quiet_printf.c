#include <stdarg.h>
#include <stdbool.h>
#include <stdio.h>
#include <string.h>

static bool keep_message(const char* format) {
    if (format == NULL) {
        return false;
    }
    return strstr(format, "Unit testing complete") != NULL
        || strstr(format, "TEST FAILED AT") != NULL;
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
