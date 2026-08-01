/*
 * Soft abort for the default-config stub gate.
 *
 * See soft_abort.h: the frozen suite's TEST_EARLY_EXIT calls abort() on the
 * first failed assertion. This ordinary function returns so the suite can
 * finish and print failure totals while stubs are incomplete.
 */
#include <stdio.h>

void mpack_soft_abort(void) {
    /* Heartbeat so hung soft-continue loops are visible under --default-config. */
    static unsigned long count;
    ++count;
    if ((count % 1000000ul) == 0ul) {
        fprintf(stderr, "[soft-abort] %lu\n", count);
    }
}
