#pragma once
#include "protocol.h"

/* Freestanding integer accounting; callers serialize each application's budget. */
typedef struct BL_BUDGET { BL_U64 rate, credit, at; } BL_BUDGET;
static __inline void BlBudgetInit(BL_BUDGET *b, BL_U64 rate, BL_U64 now) {
    b->rate = rate; b->credit = 0; b->at = now;
}
static __inline BL_U64 BlBudgetAvailable(BL_BUDGET *b, BL_U64 now) {
    BL_U64 elapsed = now >= b->at ? now - b->at : 0;
    BL_U64 capacity = b->rate * (BL_TICKS_PER_SECOND / 4);
    if (capacity < BL_MAX_SEGMENT * BL_TICKS_PER_SECOND) capacity = BL_MAX_SEGMENT * BL_TICKS_PER_SECOND;
    if (elapsed > BL_TICKS_PER_SECOND / 4) elapsed = BL_TICKS_PER_SECOND / 4;
    if (now > b->at) b->at = now;
    b->credit += elapsed * b->rate;
    if (b->credit > capacity) b->credit = capacity;
    return b->credit / BL_TICKS_PER_SECOND;
}
/* Queued indications are indivisible. Never inject a partial NBL chain. Zero is unlimited. */
static __inline int BlBudgetTakeWhole(BL_BUDGET *b, BL_U64 bytes, BL_U64 now) {
    if (b->rate == 0 || bytes == 0) return 1;
    if (BlBudgetAvailable(b, now) < bytes) return 0;
    b->credit -= bytes * BL_TICKS_PER_SECOND;
    return 1;
}
static __inline BL_U64 BlBudgetTake(BL_BUDGET *b, BL_U64 requested, BL_U64 now) {
    BL_U64 available = BlBudgetAvailable(b, now);
    BL_U64 allowed = requested < available ? requested : available;
    b->credit -= allowed * BL_TICKS_PER_SECOND;
    return allowed;
}
static __inline int BlConfigValid(const BL_CONFIG *c) {
    BL_U32 i;
    if (c->version != BL_VERSION || c->size != sizeof(*c) || c->count == 0 ||
        c->count > BL_MAX_RULES || c->lease_ms < 1000 || c->lease_ms > 30000) return 0;
    for (i = 0; i < BL_MAX_RULES; ++i) {
        BL_U64 rate = c->download_bytes_per_second[i], upload = c->upload_bytes_per_second[i];
        if (i < c->count ? ((rate == 0 && upload == 0) || rate > BL_MAX_RATE || upload > BL_MAX_RATE ||
            (rate != 0 && rate < 1024) || (upload != 0 && upload < 1024)) : (rate != 0 || upload != 0)) return 0;
    }
    return 1;
}
