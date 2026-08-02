/*
 * Identity libc wrappers + assert stubs for the Rust full-suite-abi staticlib.
 *
 * Under full-suite-abi the Rust FFI calls test_malloc / test_free by name.
 * Without --cfg mpack_frozen_link the crate embeds cargo-test shims whose
 * test_free is a no-op (leaks). We build without that cfg (so growable buffer
 * size stays 4096) and override the shim symbols at final link with these
 * libc wrappers via -Wl,--allow-multiple-definition (first definition wins).
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

void* test_malloc(size_t size) {
    return malloc(size == 0 ? 1 : size);
}

void test_free(void* pointer) {
    free(pointer);
}

size_t test_strlen(const char* value) {
    if (value == NULL) {
        return 0;
    }
    return strlen(value);
}

void mpack_assert_fail(const char* message) {
    (void)message;
    abort();
}

void mpack_break_hit(const char* message) {
    (void)message;
}

FILE* test_fopen(const char* filename, const char* mode) {
    return fopen(filename, mode);
}

int test_fclose(FILE* file) {
    return fclose(file);
}

size_t test_fread(void* data, size_t size, size_t count, FILE* file) {
    return fread(data, size, count, file);
}

size_t test_fwrite(const void* data, size_t size, size_t count, FILE* file) {
    return fwrite(data, size, count, file);
}

int test_fseek(FILE* file, long offset, int whence) {
    return fseek(file, offset, whence);
}

long test_ftell(FILE* file) {
    return ftell(file);
}

int test_ferror(FILE* file) {
    return ferror(file);
}
