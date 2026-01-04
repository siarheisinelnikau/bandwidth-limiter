#include <assert.h>
#include <stdio.h>
#include "../shared/budget.h"

int main(void) {
    BL_BUDGET b;
    BL_U64 tick, connection, total = 0;
    BL_CONFIG c = {0};
    assert(sizeof(BL_CONFIG) == 528 && sizeof(BL_STATUS) == 96);
    assert(BL_CONFIGURE == 0x22e000u && BL_QUIESCE == 0x22e014u);
    BlBudgetInit(&b, 4096, 0);
    for (tick = 0; tick <= 1000; ++tick)
        for (connection = 0; connection < 8; ++connection)
            total += BlBudgetTake(&b, 4096, tick * 100000ull);
    assert(total <= 40960 && total >= 40959);
    assert(BlBudgetTake(&b, 4096, 1) == 0); /* backward time creates no credit */
    assert(BlBudgetTake(&b, 4096, 200000000ull) == 1024); /* bounded idle burst */
    BlBudgetInit(&b, BL_MAX_RATE, 0);
    assert(BlBudgetAvailable(&b, ~0ull) == BL_MAX_RATE / 4); /* overflow bound */
    c.version = BL_VERSION; c.size = sizeof(c); c.count = 1; c.lease_ms = 5000;
    c.download_bytes_per_second[0] = 1024; assert(BlConfigValid(&c));
    c.upload_bytes_per_second[0] = 2048; assert(BlConfigValid(&c));
    { BL_BUDGET down, up; BL_U64 sent = 0, received = 0;
      BlBudgetInit(&down, 4096, 0); BlBudgetInit(&up, 8192, 0);
      for (tick = 0; tick <= 1000; ++tick) for (connection = 0; connection < 8; ++connection) {
        if (BlBudgetTakeWhole(&down, 1024, tick * 100000ull)) received += 1024;
        if (BlBudgetTakeWhole(&up, 1024, tick * 100000ull)) sent += 1024;
      }
      assert(received == 40960 && sent == 81920);
      BlBudgetInit(&up, 1024, 0);
      for (tick = 0; tick < 6400; ++tick) assert(!BlBudgetTakeWhole(&up, 65536, tick * 100000ull));
      assert(BlBudgetTakeWhole(&up, 65536, 640000000ull));
    }
    c.download_bytes_per_second[31] = 1; assert(!BlConfigValid(&c));
    c.download_bytes_per_second[31] = 0; c.count = 33; assert(!BlConfigValid(&c));
    puts("WFP shared ABI and synthetic multi-flow budget tests passed");
    return 0;
}
