/*
 * Soft-continue companion for soft_abort.h (frozen-link --soft-continue only).
 * See soft_abort.h: the frozen suite's TEST_EARLY_EXIT calls abort() on the
 * first failure; soft-continue remaps that to this returning function so a
 * full failure summary can print. Parity builds must not link this file.
 */
#include <stdio.h>

void mpack_soft_abort(void) {
    /* Heartbeat so hung soft-continue loops are visible under --soft-continue. */
    static unsigned long count;
    ++count;
    if ((count % 1000000ul) == 0ul) {
        fprintf(stderr, "[soft-abort] %lu\n", count);
    }
}
