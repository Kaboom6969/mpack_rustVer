#include <stdint.h>

#include "mpack/mpack.h"

/*
 * This intentionally uses the upstream header chain and the same public ABI
 * calls as the frozen suite. It is the first executable link checkpoint before
 * the full writer test object can resolve every MPack symbol.
 */
int main(void) {
    char buffer[1] = {0};
    mpack_writer_t writer;

    mpack_writer_init(&writer, buffer, sizeof(buffer));
    mpack_write_nil(&writer);

    if (mpack_writer_error(&writer) != mpack_ok)
        return 1;
    if ((uint8_t)buffer[0] != 0xc0)
        return 2;
    return mpack_writer_destroy(&writer) == mpack_ok ? 0 : 3;
}
